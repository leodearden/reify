/**
 * Per-cell display-unit ladder helpers (task #5199).
 *
 * Pure functions supporting the Parameters panel's per-cell unit picker:
 * converting a canonical SI magnitude into a chosen display unit, formatting
 * the result without float noise, and looking up the selectable unit ladder
 * for a cell's dimension. The ladder data itself is authoritative in Rust
 * (`reify_core::display_units`, re-exported through
 * `reify_gui::display_units`) and fetched once via the `get_unit_ladders`
 * Tauri command — these helpers only consume it.
 */

import type { UnitLadderMap, UnitOption } from '../types';

/**
 * Format a number for display, mirroring the Rust `format_display_number`
 * whole-number path plus a 12-significant-figure trim for everything else.
 *
 * The trim kills float noise from unit conversion (e.g. division by
 * `si_scale`) the same way task #5198 kills it on the backend's default-unit
 * path: round-tripping through `toPrecision(12)` discards noise beyond the
 * 12th significant figure, then `Number(...)` + `String(...)` drops
 * trailing zeros (`"0.300000000000"` -> `0.3` -> `"0.3"`).
 *
 * Non-finite values (`Infinity`, `NaN`) fall back to `String(v)` verbatim.
 */
export function formatDisplayNumber(v: number): string {
  if (!Number.isFinite(v)) return String(v);
  if (Number.isInteger(v) && Math.abs(v) < 1e15) {
    return String(Math.trunc(v));
  }
  return String(Number(v.toPrecision(12)));
}

/** Convert a canonical SI magnitude into a chosen display unit's magnitude. */
export function convertToUnit(siValue: number, siScale: number): number {
  return siValue / siScale;
}

/**
 * The ASCII normal form of a CURATED unit label (task #6028).
 *
 * Rewrites the two superscript exponent glyphs that appear in the curated
 * ladder tables — U+00B2 -> `^2`, U+00B3 -> `^3` — and touches nothing else.
 *
 * WHY it exists: task #5788 relabels the curated tables from the superscript
 * spelling (`mm³`) to the ASCII spelling (`mm^3`) per its contract C2 —
 * "accept what we cannot enumerate; normalize what we curate". A label
 * compared by raw string equality across that relabel stops matching, so a
 * user's persisted unit preference would silently snap back to the ladder
 * default. Normalizing BOTH sides before comparing makes the comparison
 * survive the relabel in EITHER direction, which is what lets #6028 land
 * before or after #5788 without an ordering hazard.
 *
 * It deliberately does NOT touch the `×10ⁿ` engineering-notation superscript
 * digits produced by `reify-ir`'s value formatting (task #5788 PRD §10 /
 * addendum L3): those format the MAGNITUDE, not the unit, and are explicitly
 * out of scope. This is a pure glyph substitution with no exceptions, so it
 * must only ever be handed a unit label.
 */
export function normalizeUnitLabel(label: string): string {
  return label.replace(/²/g, '^2').replace(/³/g, '^3');
}

/**
 * The unit labels the typed-quantity gate accepts when no ladder data is
 * available (task #6028) — the five that were hard-coded into
 * `PropertyEditor`'s `QUANTITY_RE` before the alphabet became ladder-derived.
 *
 * This is a FLOOR, not the alphabet: `get_unit_ladders` is a best-effort
 * one-shot fetch that can fail (`App.test.tsx` pins a toast for that path),
 * and validation must not silently narrow when it does. Keeping exactly these
 * five also keeps every ladder-less caller byte-identical to the pre-#6028
 * behaviour.
 */
export const BASE_UNIT_LABELS: readonly string[] = ['mm', 'cm', 'm', 'deg', 'rad'];

/**
 * The unit alphabet the typed-quantity gate accepts, derived from the LIVE
 * ladders unioned with {@link BASE_UNIT_LABELS} (task #6028).
 *
 * WHY derived rather than a hand-written mirror of the Rust curated table: a
 * TS copy would be a FIFTH curated label table, which is the exact drift
 * defect task #5788 decision D6 exists to prevent. Deriving it from the data
 * the backend already advertises means this needs no follow-up edit when
 * #5788 relabels — the alphabet just follows.
 *
 * WHY the labels are normalized here: the typed-input gate should accept what
 * the **.ri grammar** accepts, and superscript spellings have never been
 * parseable (task #5788 probe evidence). So `mm^3` becomes accepted while
 * `mm³` stays rejected, in both the pre- and post-#5788 eras.
 *
 * WHAT "advertised" DOES NOT MEAN. The ladders are the curated DISPLAY table
 * (`unit_ladders()`, crates/reify-core/src/display_units.rs), which is a
 * strictly LARGER set than what the commit path can input-parse. That path —
 * `handleSetParameter` (App.tsx) -> `bridge.setParameter` ->
 * `EngineSession::set_parameter` (gui/src-tauri/src/engine.rs:2051-2057) ->
 * `parse_value_string` (:5932) — matches only `UNIT_TABLE` (:5918-5924), a
 * hard-coded five-entry suffix table (`deg`, `rad`, `mm`, `cm`, `m`) built
 * from neither the .ri unit registry nor `unit_ladders()`. So deriving the
 * alphabet from the ladders deliberately admits 19 labels the backend then
 * refuses — `in`, `mm^2`, `cm^2`, `m^2`, `mm^3`, `cm^3`, `L`, `m^3`, `g`,
 * `kg`, `Pa`, `kPa`, `MPa`, `GPa`, `kg/m^3`, `g/cm^3`, `N`, `J`, `W` — with
 * `Cannot parse value '<input>'` (engine.rs:5973), not the .ri compiler's
 * unknown-unit diagnostic (a different subsystem this path never reaches).
 *
 * The user-visible cost is that the typed text is discarded and an async
 * error toast replaces the inline `data-invalid` that used to preserve it for
 * correction. Accepted, and pinned end-to-end by the `App.test.tsx` block
 * "ladder-derived units the backend cannot parse"; the caller-side rationale
 * lives on `quantityRe` in `../panels/PropertyEditor.tsx`. Task #5757 widens
 * `UNIT_TABLE` but is necessary-not-sufficient — it reconciles against
 * `reify_core::units::unit_symbol_to_si` (crates/reify-core/src/units.rs:24-51),
 * 13 BARE symbols with no compound units — so the compound half is filed as
 * an agent-followup ticket spawned from #6028 naming #5757 as its sibling.
 * Task #5788 fixes none of it: it touches the compiler's stdlib registry, not
 * `gui/src-tauri`.
 */
export function quantityUnitAlphabet(ladders: UnitLadderMap | undefined): string[] {
  const alphabet = new Set<string>(BASE_UNIT_LABELS);
  for (const options of Object.values(ladders ?? {})) {
    // `options` and `opt.label` are typed non-null/string, but this data
    // crosses an IPC boundary (`get_unit_ladders`), so the types are a claim
    // about the backend's serde shape rather than a runtime guarantee. Before
    // #6028 a malformed payload could only degrade the unit PICKER — the
    // label lookup simply missed. Now the same payload feeds typed-value
    // validation, so an unguarded `label.replace` would throw out of the
    // `quantityRe` memo on EVERY Enter/blur in the panel, for every cell.
    // Skipping the bad entry keeps the blast radius where it was.
    for (const opt of options ?? []) {
      if (typeof opt?.label === 'string') alphabet.add(normalizeUnitLabel(opt.label));
    }
  }
  return [...alphabet];
}

/**
 * Escape every regex metacharacter in a unit label so it matches literally.
 *
 * `^` is the one that matters and the one that bites: inside an alternation
 * branch an unescaped `^` is a START-OF-INPUT ANCHOR, so a `mm^3` branch would
 * compile to "mm, then start-of-input, then 3" — a perfectly valid regex that
 * can never match. Nothing throws and nothing warns; the gate just silently
 * rejects every `mm^3` a user types. `/` matters too, for compound labels like
 * `kg/m^3`.
 */
function escapeForRegex(literal: string): string {
  return literal.replace(/[.*+?^${}()|[\]\\/-]/g, '\\$&');
}

/** The signed numeric literal of a quantity: sign, mantissa, optional exponent. */
const QUANTITY_NUMBER = '-?(?:\\d+\\.?\\d*|\\.\\d+)(?:[eE][+-]?\\d+)?';

/**
 * A bare numeric literal — the same grammar as a quantity's numeric part, with
 * no unit suffix (task #6028).
 *
 * Built from {@link QUANTITY_NUMBER} rather than restated, so the unit-less and
 * unit-suffixed input paths cannot diverge: a future decision about the numeric
 * form (accept a leading `+`, accept digit separators, …) is made once and
 * lands on both. `PropertyEditor` used to carry a byte-for-byte copy of this
 * regex next to its quantity check, which is the same sync obligation #6028
 * removed from the unit alternation.
 *
 * As with {@link buildQuantityRe} there is no numeric range check, so callers
 * must still guard with `Number.isFinite(Number(value))` — `1e999` matches and
 * converts to `Infinity`.
 */
export const NUMBER_RE = new RegExp(`^(${QUANTITY_NUMBER})$`);

/**
 * Build the typed-quantity regex for a unit alphabet (task #6028) — the ONE
 * definition of the quantity grammar in the frontend. Before this, the
 * five-unit alternation was written four times across
 * `PropertyEditor.tsx` and `PropertyEditor.test.tsx`, each copy carrying its
 * own obligation to stay in sync.
 *
 * **Capture group 1 is the whole signed numeric literal** — sign, mantissa and
 * exponent. That is what collapses the old pair of regexes into one: the
 * caller reads `m[1]` for the overflow check instead of re-declaring the
 * alternation to strip the unit suffix.
 *
 * Labels are sorted longest-first with a lexicographic tiebreak purely for
 * DETERMINISM — the output must not depend on ladder iteration order. Match
 * correctness itself comes from the `$` anchor, not from branch order.
 *
 * Grammar notes, all deliberate and all inherited from the regex this
 * replaces: no whitespace is allowed between the number and the unit,
 * mirroring the .ri grammar's `token.immediate`
 * (`tree-sitter-reify/grammar.js`) — the backend's `parse_value_string` is
 * more lenient (it accepts `"5 mm"`) but the frontend intentionally enforces
 * the stricter rule. A leading `+` is rejected because the grammar defines
 * only unary minus for number literals, even though the exponent does accept
 * a sign (`1e+3mm` is valid). There is no numeric range check, so callers must
 * still guard with `Number.isFinite(Number(m[1]))`.
 */
export function buildQuantityRe(labels: readonly string[]): RegExp {
  const ordered = [...labels].sort((a, b) => b.length - a.length || (a < b ? -1 : a > b ? 1 : 0));
  const alternation = ordered.map(escapeForRegex).join('|');
  return new RegExp(`^(${QUANTITY_NUMBER})(?:${alternation})$`);
}

/** Look up the selectable unit ladder for a canonical dimension name. */
export function ladderForDimension(
  map: UnitLadderMap,
  dimension: string,
): UnitOption[] | undefined {
  return map[dimension];
}
