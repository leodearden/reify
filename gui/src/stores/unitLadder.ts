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
 * THE CANONICAL RATIONALE FOR THE WHOLE SEAM LIVES HERE. Callers
 * (`quantityReFor` in `../panels/PropertyEditor.tsx`, the `App.test.tsx`
 * block "ladder-derived units the backend cannot parse") point at this block
 * rather than restating it — three copies of the same argument, each naming
 * its own Rust line numbers, was three sets of rot sites with no gate to catch
 * them. For the same reason everything below names Rust SYMBOLS, never line
 * numbers, and describes sets by RULE, never by enumeration: a hand-mirrored
 * snapshot of the curated table in comment form is the very drift defect this
 * function exists to avoid in data.
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
 * WHAT "ADVERTISED" NOW MEANS — the gap this used to document is CLOSED
 * (task #5757). The ladders are the curated DISPLAY table
 * (`reify_core::display_units::unit_ladders`), and until #5757 the commit path
 * — `handleSetParameter` (App.tsx) -> `bridge.setParameter` ->
 * `EngineSession::set_parameter` -> `parse_value_string` (both in
 * gui/src-tauri/src/engine.rs) — matched a hard-coded five-entry suffix table
 * whose entries were exactly {@link BASE_UNIT_LABELS}. Every curated label
 * outside that floor was admitted here and then refused on commit with
 * `Cannot parse value '<input>'`, discarding the typed text behind an async
 * toast.
 *
 * `parse_value_string` now scans an index composed from THIS SAME
 * `unit_ladders()` table unioned with `reify_core::BUILTIN_UNITS`, so the two
 * ends are two readers of one table rather than two tables kept in lockstep.
 * The backend registers each rung under BOTH its raw superscript spelling and
 * its {@link normalizeUnitLabel} form, while this gate admits only the ASCII
 * one — so the backend is a strict SUPERSET of what this alphabet can produce,
 * and no label admitted here can be refused on commit. Compound labels
 * (`mm^3`, `kg/m^3`, `g/cm^3`) are included: the backend matches them as whole
 * suffixes off the composed index, so it never has to compose a `UnitExpr` the
 * way the .ri grammar does.
 *
 * THE ONE ASYMMETRY THAT REMAINS runs the other way and is deliberate: the
 * backend also accepts spellings this gate refuses — raw superscripts, the SI
 * bases no ladder carries (`s`, `K`, `A`, `mol`, `cd`), and a label belonging
 * to a dimension other than the cell's. The last of those is not silently
 * taken: it resolves to its own dimension and is then refused by reify-eval
 * with a `DimensionMismatch` naming both. Callers still scope this alphabet to
 * the cell's own dimension, so the panel rejects a cross-dimension literal
 * inline and the backend path is only reached by callers that bypass this gate
 * (`MechanismPanel`, via `handleSetParameter`).
 *
 * Separately, {@link acceptsBareNumber} refuses a bare number for a cell whose
 * dimension THIS alphabet can express — i.e. one with a curated ladder —
 * mirroring the backend's `parse_value_string_for_cell`. The two are the same
 * question asked twice, which is why they read the same map: a cell may only be
 * told to supply a unit when a unit is on offer for it. The `App.test.tsx`
 * block that used to pin the degradation above now pins this reconciled
 * contract in both directions.
 *
 * Still deferred, and NOT this: compound unit EXPRESSIONS in .ri source at the
 * `bind(joint, <quantity>)` site (`UnitExpr::Mul`/`Div`/`Pow`), which are task
 * γ (#3803); and the compiler's per-module `UnitRegistry` spellings (`km`,
 * `ft`, `psi`, `degC`, …), which this gate rejects outright and which are
 * therefore unreachable from the property editor.
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
 * Whether a cell with this dimension accepts a BARE number as typed input
 * (task #5757).
 *
 * THE RULE: a cell needs a unit exactly when one CAN be typed for it. `20` in a
 * `Volume` cell is ambiguous — 20 what? — and the engine used to resolve that
 * ambiguity by silently reading it as 20 CUBIC METRES, the same 1000× hazard
 * the .ri geometry-argument gate rejects with "pass a dimensioned length such
 * as `5mm`". This is the panel's inline mirror of that rule.
 *
 * EXPRESSIBILITY IS THE KEY, NOT DIMENSIONEDNESS. That ambiguity presupposes a
 * unit COULD have been typed: `20` is ambiguous in a Volume cell precisely
 * because `20mm^3` and `20L` were both on offer. For a dimension no curated
 * ladder covers, the picker offers nothing and {@link quantityUnitAlphabet}
 * admits nothing, so refusing the bare number disambiguates nothing — it
 * removes the cell's LAST accepted input and bricks the row.
 *
 * The concrete case this was breaking: `Money`. Sixteen `param … : Money`
 * declarations across `examples/*.ri` spell their literals `NUSD`, and `USD` is
 * reachable only through the compiler's per-module `UnitRegistry`, which the
 * engine's `COMPOSED_UNIT_INDEX` deliberately excludes — so neither `6` nor
 * `6USD` was accepted and the cell could not be edited at all.
 *
 * Not a weakening, and the guarantee that replaces the old one is stronger for
 * being checkable: GATED ⟺ a rung exists in this cell's own ladder ⟺ the
 * picker and the alphabet can express it. It reuses {@link ladderForDimension}
 * rather than indexing the map, so "what counts as covered" has ONE definition,
 * shared with `pickerLadder`, `editSeedUnitLabel` and `quantityReFor`. It still
 * enumerates no unit strings — only map membership — so the standing #5788 D6
 * prohibition documented on {@link quantityUnitAlphabet} is untouched.
 *
 * THE BACKEND IS THE AUTHORITATIVE GATE and agrees exactly:
 * `parse_value_string_for_cell` in `gui/src-tauri/src/engine.rs` refuses a
 * `Value::Int`/`Value::Real` only for a dimension its `LADDER_COVERAGE` table
 * records, and does so for every caller of `set_parameter` — including
 * `MechanismPanel`, which reaches `handleSetParameter` without passing through
 * `PropertyEditor`'s gate. This predicate exists to make the refusal INLINE,
 * keeping the typed text on screen for correction instead of discarding it
 * behind an async error toast.
 *
 * IT FAILS OPEN. With `ladders` undefined or empty — the `get_unit_ladders`
 * fetch not resolved, or failed — nothing is expressible, so nothing is gated.
 * That is the safe direction: the backend stays authoritative, and
 * over-rejecting here would discard input the engine would have accepted. It
 * adds no new degradation either, since {@link quantityUnitAlphabet} already
 * collapses to {@link BASE_UNIT_LABELS} on that same path.
 *
 * IT DOES NOT COST THE ORDINARY EDIT A UNIT KEYSTROKE. `PropertyEditor`'s
 * `editSeed` seeds a COVERED cell's input with a unit-bearing literal
 * (magnitude + the cell's default ladder rung), so committing an untouched row
 * is still a no-op and changing only the digits still submits a united literal.
 * A predicate like this one is only safe to add alongside a seed like that. An
 * UNCOVERED cell seeds the bare magnitude, which is now consistent rather than
 * a degradation: that seed is a literal both ends accept.
 *
 * THE ASYMMETRY THAT REMAINS runs the documented safe direction only — the
 * backend accepts spellings this gate refuses (raw superscripts, the SI bases
 * no ladder carries: `s`, `K`, `A`, `mol`, `cd`, and cross-dimension labels).
 * A COMPOSED dimension is no longer part of it: the backend serialises one as
 * the empty string, indistinguishable here from dimensionless, and since the
 * coverage-conditional rule the backend does not gate it either — so the two
 * ends now AGREE on it.
 */
export function acceptsBareNumber(
  dimension: string | undefined,
  ladders: UnitLadderMap | undefined,
): boolean {
  return !dimension || ladderForDimension(ladders ?? {}, dimension) === undefined;
}

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
