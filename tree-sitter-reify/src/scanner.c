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
 *                         _auto_token(3), _auto_reservation_sentinel(4),
 *                         _radix_literal(5)] */
enum TokenType {
  UNIT_EXPR_START,            /* index 0 — zero-width quantity-literal gate    */
  UNIT_MUL_OP,                /* index 1 — '*' inside unit_expr                */
  UNIT_DIV_OP,                /* index 2 — '/' inside unit_expr                */
  AUTO_TOKEN,                 /* index 3 — the bare 'auto' keyword token        */
  AUTO_RESERVATION_SENTINEL,  /* index 4 — NEVER emitted; keeps scanner active.
                               *
                               * TRIPWIRE: if you remove AUTO_RESERVATION_SENTINEL
                               * from grammar.js `extras`, the external scanner will
                               * no longer be invoked at operand positions.  `auto`
                               * will silently lex as `identifier` there, and the
                               * operand-rejection tests will pass for the WRONG
                               * reason (no ERROR at all, not a parse failure).
                               * Removal is caught by:
                               *   - auto_operand_rejection.txt (:error fixtures)
                               *   - section C of auto_binding_sites_grammar_tests.rs
                               */
  RADIX_LITERAL,              /* index 5 — 0x.../0b... integer literal          */
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

/* Helper: returns true iff c is a valid hexadecimal digit [0-9a-fA-F]. */
static bool is_hex_digit(int32_t c) {
  return (c >= '0' && c <= '9') || (c >= 'a' && c <= 'f') ||
         (c >= 'A' && c <= 'F');
}

/* Helper: returns true iff c is a valid binary digit [01]. */
static bool is_bin_digit(int32_t c) {
  return c == '0' || c == '1';
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

      /* D1 IMAGINARY-LITERAL GATE (PRD v0_6 complex-literals-and-stdmath):
       * When the lookahead is a bare lowercase `j` NOT followed by a word
       * character ([A-Za-z0-9_]), refuse UNIT_EXPR_START so that the grammar's
       * token.immediate('j') can match instead, forming an imaginary_literal.
       *
       * mark_end is called BEFORE the lookahead peek so UNIT_EXPR_START stays
       * zero-width regardless of how far we advance to inspect the char after `j`.
       * Multi-char j-units (jk, joule, ...) and capital J (Joule) fall through
       * the normal path — only a lone lowercase `j` triggers this gate. */
      lexer->mark_end(lexer); /* zero-width: fix boundary before any peek */
      if (c == 'j') {
        lexer->advance(lexer, false);
        int32_t after = lexer->lookahead;
        bool is_word = (after >= 'a' && after <= 'z') ||
                       (after >= 'A' && after <= 'Z') ||
                       (after >= '0' && after <= '9') ||
                       (after == '_');
        if (!is_word) {
          /* Bare `j`: refuse UNIT_EXPR_START; let token.immediate('j') match. */
          return false;
        }
        /* Multi-char j-unit (jk, joule, ...): fall through to emit UNIT_EXPR_START. */
      }

      lexer->result_symbol = UNIT_EXPR_START;
      return true;
    }
    /* Whitespace or non-unit-start: fall through to AUTO_TOKEN check. */
  }

  /* ── RADIX_LITERAL: 0x.../0b... integer literals ──────────────────────────
   *
   * Emits RADIX_LITERAL (consuming the whole radix-literal run) when:
   *   1. The parser is requesting it (valid_symbols[RADIX_LITERAL]).
   *   2. After skipping leading whitespace, the lookahead is `0`.
   *   3. The char after `0` is a radix prefix: `x`/`X` (hex) or `b`/`B` (binary).
   *   4. At least one radix digit follows the prefix.
   *
   * DIGIT CONSUMING: additional digits are consumed greedily with `_`-separator
   * support via the pattern (digit | `_` digit)*.  INCREMENTAL mark_end after
   * each complete digit ensures a trailing `_` (e.g. `0xFF_`) is NOT absorbed.
   *
   * FALL-THROUGH on non-`0`: if the first non-whitespace char is NOT `0`, we do
   * NOT return false — instead we fall through to the AUTO_TOKEN block so that
   * `auto` at binding sites still lexes correctly (both RADIX_LITERAL and
   * AUTO_TOKEN can be valid at the same parser state).
   *
   * RETURN-FALSE on 0-not-radix: if `0` is followed by something other than a
   * valid radix prefix+digit, we return false.  Tree-sitter's "advance + return
   * false rewinds all advances" contract ensures the decimal DFA token then lexes
   * `0`, `0.5`, `0e5`, etc.
   *
   * STOP AT NON-DIGIT: the scanner stops at the first non-radix char (e.g. `m`
   * in `0xFFmm`), leaving the rest for the unit_expr machinery to handle.
   */
  if (valid_symbols[RADIX_LITERAL]) {
    /* Skip leading whitespace — scanner fires at the ws position before `0`. */
    int32_t rch = lexer->lookahead;
    while (rch == ' ' || rch == '\t' || rch == '\n' || rch == '\r') {
      lexer->advance(lexer, true); /* skip=true: mark whitespace as skipped */
      rch = lexer->lookahead;
    }

    if (rch != '0') {
      /* Not a radix literal — fall through to AUTO_TOKEN block. */
      goto auto_token_block;
    }

    /* Consume the leading `0`. */
    lexer->advance(lexer, false);
    rch = lexer->lookahead;

    /* Determine radix from prefix char. */
    bool is_hex = (rch == 'x' || rch == 'X');
    bool is_bin = (rch == 'b' || rch == 'B');

    if (!is_hex && !is_bin) {
      /* `0` not followed by a valid radix prefix (e.g. `0.5`, `0`, `0e5`).
       * Return false — tree-sitter rewinds all advances and the decimal DFA
       * token lexes the literal. */
      return false;
    }

    /* Consume the prefix char (`x`/`X` or `b`/`B`). */
    lexer->advance(lexer, false);
    rch = lexer->lookahead;

    /* Require at least one valid digit immediately after the prefix. */
    bool first_digit = is_hex ? is_hex_digit(rch) : is_bin_digit(rch);
    if (!first_digit) {
      /* Prefix with no digits (e.g. `0x` or `0b` alone) — return false so
       * tree-sitter rewinds.  These parse as quantity_literal(number_literal "0",
       * unit_expr "x"/"b") with no error node per task spec. */
      return false;
    }

    /* Consume digits with `_`-separator support.
     * Pattern: digit (_? digit)* — use INCREMENTAL mark_end after each digit
     * so a trailing `_` is NOT absorbed into the token. */
    lexer->advance(lexer, false);
    lexer->mark_end(lexer); /* mark after first digit */
    rch = lexer->lookahead;

    for (;;) {
      if (rch == '_') {
        /* Peek ahead: only consume `_` if a valid digit follows it. */
        lexer->advance(lexer, false); /* tentatively consume `_` */
        rch = lexer->lookahead;
        bool next_ok = is_hex ? is_hex_digit(rch) : is_bin_digit(rch);
        if (!next_ok) {
          /* Trailing `_` — do NOT update mark_end; stop here.
           * Tree-sitter will rewind the `_` advance (return true uses mark_end). */
          break;
        }
        /* Valid `_digit` pair — consume the digit and advance mark_end. */
        lexer->advance(lexer, false);
        lexer->mark_end(lexer);
        rch = lexer->lookahead;
      } else if (is_hex ? is_hex_digit(rch) : is_bin_digit(rch)) {
        lexer->advance(lexer, false);
        lexer->mark_end(lexer);
        rch = lexer->lookahead;
      } else {
        break;
      }
    }

    lexer->result_symbol = RADIX_LITERAL;
    return true;
  }

  /* ── AUTO_TOKEN: narrow lexer-level reservation of `auto` ─────────────────
   *
   * Label used as a fall-through target from the RADIX_LITERAL block when the
   * first non-whitespace char is not `0`.  C89 requires a statement after a
   * label, so the opening brace of the existing block serves as that statement. */
  auto_token_block:
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

    /* Early exit: the vast majority of tokens start with something other than
     * 'a', so this is the hot path.  The per-position scanner overhead is
     * minimal — one whitespace-skip loop + one character comparison. */
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
