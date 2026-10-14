import { describe, it, expect } from 'vitest';
import type { UnitLadderMap } from '../types';
import {
  formatDisplayNumber,
  convertToUnit,
  ladderForDimension,
  normalizeUnitLabel,
  quantityUnitAlphabet,
  BASE_UNIT_LABELS,
  BASE_UNIT_DIMENSIONS,
  buildQuantityRe,
  NUMBER_RE,
  acceptsBareNumber,
} from '../stores/unitLadder';

describe('formatDisplayNumber', () => {
  it('formats a plain decimal verbatim', () => {
    expect(formatDisplayNumber(7.04500224)).toBe('7.04500224');
  });

  it('trims 12-sig-fig float noise from a division result', () => {
    expect(formatDisplayNumber(7045002.24 / 1e6)).toBe('7.04500224');
  });

  it('kills classic 0.1 + 0.2 float noise', () => {
    expect(formatDisplayNumber(0.1 + 0.2)).toBe('0.3');
  });

  it('formats a whole number without a decimal point', () => {
    expect(formatDisplayNumber(80)).toBe('80');
  });

  it('formats a negative decimal', () => {
    expect(formatDisplayNumber(-3.5)).toBe('-3.5');
  });

  it('takes the whole-number path for a large integral value', () => {
    expect(formatDisplayNumber(1e15)).toBe('1000000000000000');
  });

  it('falls back to String(v) for non-finite values', () => {
    expect(formatDisplayNumber(Infinity)).toBe(String(Infinity));
    expect(formatDisplayNumber(NaN)).toBe(String(NaN));
  });
});

describe('convertToUnit', () => {
  it('converts an SI volume magnitude to litres', () => {
    expect(convertToUnit(0.00704500224, 1e-3)).toBeCloseTo(7.04500224, 9);
  });

  it('converts an SI length magnitude to millimetres', () => {
    expect(convertToUnit(0.08, 1e-3)).toBeCloseTo(80, 9);
  });
});

describe('ladderForDimension', () => {
  const map = {
    Volume: [
      { label: 'mm^3', si_scale: 1e-9, is_default: true },
      { label: 'L', si_scale: 1e-3, is_default: false },
    ],
  };

  it('returns the option list for a known dimension', () => {
    expect(ladderForDimension(map, 'Volume')).toBe(map.Volume);
  });

  it('returns undefined for an unknown dimension', () => {
    expect(ladderForDimension(map, 'Force')).toBeUndefined();
  });
});

/**
 * Task #6028: the ASCII normal form of a curated unit label.
 *
 * Superscripts are written as `²`/`³` escapes here (mirroring the
 * escape convention on the Rust side, task #5788 addendum L2) so the
 * expectations stay greppable and cannot be silently mangled by an editor or
 * a locale-dependent normalisation pass.
 */
describe('normalizeUnitLabel', () => {
  it.each([
    ['mm²', 'mm^2'],
    ['cm²', 'cm^2'],
    ['m²', 'm^2'],
  ])('rewrites U+00B2 to ^2: %s -> %s', (input, expected) => {
    expect(normalizeUnitLabel(input)).toBe(expected);
  });

  it.each([
    ['mm³', 'mm^3'],
    ['cm³', 'cm^3'],
    ['m³', 'm^3'],
    ['kg/m³', 'kg/m^3'],
    ['g/cm³', 'g/cm^3'],
  ])('rewrites U+00B3 to ^3: %s -> %s', (input, expected) => {
    expect(normalizeUnitLabel(input)).toBe(expected);
  });

  it.each([['mm'], ['L'], ['deg'], ['rad'], ['kPa'], ['kg/m^3'], ['mm^3'], ['']])(
    'is the identity on a label carrying no superscript: %s',
    (input) => {
      expect(normalizeUnitLabel(input)).toBe(input);
    },
  );

  it.each([
    ['mm³'],
    ['kg/m³'],
    ['m²'],
    ['mm'],
    ['L'],
  ])('is idempotent: normalize(normalize(%s)) === normalize(%s)', (input) => {
    const once = normalizeUnitLabel(input);
    expect(normalizeUnitLabel(once)).toBe(once);
  });

  it('replaces EVERY occurrence, not just the first', () => {
    expect(normalizeUnitLabel('m³/m³')).toBe('m^3/m^3');
    expect(normalizeUnitLabel('m²·m³')).toBe('m^2·m^3');
  });

  it('rewrites ONLY the two exponent glyphs — nothing else is special-cased', () => {
    // A middle dot is structural in a compound label and must survive verbatim.
    expect(normalizeUnitLabel('N·m')).toBe('N·m');
    // No special case for engineering notation either: this is a pure glyph
    // substitution, so a magnitude string would have its exponent rewritten
    // too. That is exactly why callers must only feed it UNIT labels — the
    // `x10^n` superscript digits in `reify-ir` are magnitude formatting and
    // are out of scope (task #5788 PRD §10 / addendum L3).
    expect(normalizeUnitLabel('×10³')).toBe('×10^3');
    // Superscript digits OTHER than 2 and 3 (U+00B9 one, U+2074 four) are not
    // curated unit-label glyphs and are deliberately left alone.
    expect(normalizeUnitLabel('m¹')).toBe('m¹');
    expect(normalizeUnitLabel('m⁴')).toBe('m⁴');
  });
});

/**
 * Task #6028: the unit alphabet the typed-quantity gate accepts, DERIVED from
 * the live ladders rather than hand-mirrored from the Rust curated table.
 */
describe('quantityUnitAlphabet', () => {
  /** The same Volume+Density ladders in both spellings — pre- and post-#5788. */
  const SUPERSCRIPT_LADDERS: UnitLadderMap = {
    Volume: [
      { label: 'mm³', si_scale: 1e-9, is_default: true },
      { label: 'cm³', si_scale: 1e-6, is_default: false },
      { label: 'L', si_scale: 1e-3, is_default: false },
      { label: 'm³', si_scale: 1.0, is_default: false },
    ],
    Density: [
      { label: 'kg/m³', si_scale: 1.0, is_default: true },
      { label: 'g/cm³', si_scale: 1000.0, is_default: false },
    ],
  };

  const ASCII_LADDERS: UnitLadderMap = {
    Volume: [
      { label: 'mm^3', si_scale: 1e-9, is_default: true },
      { label: 'cm^3', si_scale: 1e-6, is_default: false },
      { label: 'L', si_scale: 1e-3, is_default: false },
      { label: 'm^3', si_scale: 1.0, is_default: false },
    ],
    Density: [
      { label: 'kg/m^3', si_scale: 1.0, is_default: true },
      { label: 'g/cm^3', si_scale: 1000.0, is_default: false },
    ],
  };

  // (a) The ladders-unavailable floor. `get_unit_ladders` is a best-effort
  // one-shot fetch that can fail (App.test.tsx pins a toast for that path), so
  // validation must not silently narrow when it does — and every existing
  // ladder-less validation test must stay byte-identical.
  it.each([
    ['undefined (fetch not resolved / failed)', undefined],
    ['an empty map', {}],
  ])('returns exactly the static baseline for %s', (_desc, ladders) => {
    expect(new Set(quantityUnitAlphabet(ladders as UnitLadderMap | undefined))).toEqual(
      new Set(['mm', 'cm', 'm', 'deg', 'rad']),
    );
  });

  it('exposes that same baseline as BASE_UNIT_LABELS', () => {
    expect([...BASE_UNIT_LABELS]).toEqual(['mm', 'cm', 'm', 'deg', 'rad']);
  });

  it('pins BASE_UNIT_DIMENSIONS — the dimensions of those same labels', () => {
    // Pinned by CONTENT, not by iterating it. Every other `BASE_UNIT_DIMENSIONS`
    // assertion in this file loops over the constant itself, so it is tautological
    // with respect to what the constant holds: dropping `'Angle'` would fail
    // nothing here, while the ladder-less panel would then accept `90` in an Angle
    // cell that `set_parameter` refuses — the panel-accepts/engine-refuses
    // regression task #5757 closed, re-entered on the degraded path.
    //
    // The cross-end direction (everything gated here is gated by the engine too)
    // is `every_dimension_the_frontend_floor_gates_is_gated_here_too` in
    // `gui/src-tauri/src/tests/engine_tests.rs`. That guard hand-mirrors these
    // pairs on its own side, so this assertion is what makes a one-sided edit
    // loud on the side that OWNS the value.
    expect([...BASE_UNIT_DIMENSIONS].sort()).toEqual(['Angle', 'Length']);
  });

  // (b) Baseline UNION the ladders' labels, ASCII-normalized.
  it('unions the baseline with the ladder labels, normalized to ASCII', () => {
    expect(new Set(quantityUnitAlphabet(SUPERSCRIPT_LADDERS))).toEqual(
      new Set([
        'mm', 'cm', 'm', 'deg', 'rad',
        'mm^3', 'cm^3', 'L', 'm^3',
        'kg/m^3', 'g/cm^3',
      ]),
    );
  });

  // THE property that makes this survive #5788 landing on either side: the two
  // spellings of the same curated ladder yield the SAME alphabet.
  it('yields an identical alphabet for the superscript and ASCII spellings', () => {
    expect(new Set(quantityUnitAlphabet(SUPERSCRIPT_LADDERS))).toEqual(
      new Set(quantityUnitAlphabet(ASCII_LADDERS)),
    );
  });

  // (c) No superscript survives into the alphabet — the typed-input gate must
  // accept what the .ri grammar accepts, and superscript spellings have never
  // been parseable.
  it.each([
    ['the superscript spelling', SUPERSCRIPT_LADDERS],
    ['the ASCII spelling', ASCII_LADDERS],
  ])('emits no U+00B2 or U+00B3 entry for %s', (_desc, ladders) => {
    for (const label of quantityUnitAlphabet(ladders)) {
      expect(label).not.toContain('²');
      expect(label).not.toContain('³');
    }
  });

  // (d) Duplicates across dimensions (and against the baseline) collapse.
  it('collapses duplicate labels across dimensions and the baseline', () => {
    const withOverlap: UnitLadderMap = {
      Length: [
        { label: 'mm', si_scale: 1e-3, is_default: true },
        { label: 'm', si_scale: 1.0, is_default: false },
      ],
      Volume: [
        { label: 'm³', si_scale: 1.0, is_default: true },
        { label: 'L', si_scale: 1e-3, is_default: false },
      ],
    };
    const alphabet = quantityUnitAlphabet(withOverlap);
    expect(new Set(alphabet).size).toBe(alphabet.length);
    expect(alphabet.filter((u) => u === 'm')).toHaveLength(1);
    expect(alphabet.filter((u) => u === 'mm')).toHaveLength(1);
  });

  // (e) Malformed ladder data must not escalate from "the picker looks wrong"
  // to "typed-value validation throws". This payload crosses an IPC boundary,
  // so `UnitOption.label: string` is a claim about the backend's serde shape,
  // not a runtime guarantee — and this alphabet feeds `isValidValue`, which
  // runs on every Enter/blur in the panel.
  it('skips ladder entries whose label is not a string, without throwing', () => {
    const malformed = {
      Volume: [
        { label: 'mm^3', si_scale: 1e-9, is_default: true },
        { label: undefined, si_scale: 1e-6, is_default: false },
        { label: null, si_scale: 1e-3, is_default: false },
        { label: 42, si_scale: 1.0, is_default: false },
      ],
    } as unknown as UnitLadderMap;
    expect(() => quantityUnitAlphabet(malformed)).not.toThrow();
    expect(new Set(quantityUnitAlphabet(malformed))).toEqual(
      new Set(['mm', 'cm', 'm', 'deg', 'rad', 'mm^3']),
    );
  });

  it('tolerates a dimension whose option list is missing entirely', () => {
    const malformed = { Volume: undefined } as unknown as UnitLadderMap;
    expect(() => quantityUnitAlphabet(malformed)).not.toThrow();
    expect(new Set(quantityUnitAlphabet(malformed))).toEqual(
      new Set(['mm', 'cm', 'm', 'deg', 'rad']),
    );
  });

  it('collapses two spellings of the same label into one entry', () => {
    const bothSpellings: UnitLadderMap = {
      Volume: [
        { label: 'm³', si_scale: 1.0, is_default: true },
        { label: 'm^3', si_scale: 1.0, is_default: false },
      ],
    };
    expect(quantityUnitAlphabet(bothSpellings).filter((u) => u === 'm^3')).toHaveLength(1);
  });
});

/**
 * Task #6028: the ONE definition of the bare-numeric grammar, shared with the
 * quantity grammar's numeric part. `PropertyEditor.tsx` and
 * `PropertyEditor.test.tsx` each used to carry a byte-for-byte copy.
 */
describe('NUMBER_RE', () => {
  it.each([
    ['80', 'plain integer'],
    ['0', 'zero'],
    ['-10', 'negative integer'],
    ['1.5', 'decimal'],
    ['1.', 'trailing dot'],
    ['.5', 'leading dot'],
    ['1e3', 'exponent'],
    ['1e+3', 'signed exponent'],
    ['-1.5E-2', 'negative mantissa, negative exponent, uppercase E'],
  ])('accepts %s (%s)', (literal) => {
    expect(NUMBER_RE.test(literal)).toBe(true);
  });

  it.each([
    ['', 'empty'],
    ['+10', 'leading plus — the grammar defines only unary minus'],
    ['.', 'bare dot'],
    ['1e', 'dangling exponent'],
    ['abc', 'not a number'],
    ['80mm', 'a quantity, not a bare number'],
    ['mm', 'a bare unit'],
    [' 80', 'leading whitespace'],
    ['80 ', 'trailing whitespace'],
    ['Infinity', 'the JS literal — Number() would accept it, the grammar does not'],
  ])('rejects %s (%s)', (literal) => {
    expect(NUMBER_RE.test(literal)).toBe(false);
  });

  // The anti-drift property, and the whole reason this is exported rather than
  // restated at the call site: the bare-number path and the quantity path use
  // the SAME numeric form, so a future change to one cannot silently miss the
  // other. Every literal NUMBER_RE accepts is exactly what a quantity's capture
  // group 1 yields when that literal is suffixed with a unit.
  it.each([['80'], ['-10'], ['.5'], ['1.'], ['1e+3'], ['-1.5E-2'], ['1e999']])(
    'agrees with buildQuantityRe capture group 1 on %s',
    (literal) => {
      expect(NUMBER_RE.test(literal)).toBe(true);
      expect(buildQuantityRe(['mm']).exec(`${literal}mm`)![1]).toBe(literal);
    },
  );

  // Same contract as buildQuantityRe: syntax only, no range check. The
  // `Number.isFinite` guard in `isValidValue` is load-bearing on BOTH paths.
  it('accepts an overflowing literal, leaving the range check to the caller', () => {
    expect(NUMBER_RE.test('1e999')).toBe(true);
    expect(Number.isFinite(Number('1e999'))).toBe(false);
  });
});

/**
 * Task #6028: the ONE definition of the typed-quantity grammar. Before this,
 * the five-unit alternation was written FOUR times across
 * `PropertyEditor.tsx` (x2) and `PropertyEditor.test.tsx` (x2).
 */
describe('buildQuantityRe', () => {
  // THE CRITICAL CASE. Inside an alternation branch an unescaped `^` is a
  // START-OF-INPUT ANCHOR, so a `mm^3` branch would compile to "mm, then
  // start-of-input, then 3" — a regex that is perfectly valid and can never
  // match. The failure is entirely silent (no throw, no warning), so it gets
  // its own explicit pin rather than riding along in the table below.
  it('escapes ^ in a label — an unescaped ^ is an anchor and would never match', () => {
    expect(buildQuantityRe(['mm^3']).test('5mm^3')).toBe(true);
    expect(buildQuantityRe(['m^2']).test('1.5m^2')).toBe(true);
  });

  it('escapes / in a label so a compound unit survives', () => {
    expect(buildQuantityRe(['kg/m^3']).test('7.8kg/m^3')).toBe(true);
    expect(buildQuantityRe(['g/cm^3']).test('0.5g/cm^3')).toBe(true);
  });

  it.each([
    ['.', ['a.c'], '5a.c', '5abc'],
    ['*', ['a*'], '5a*', '5aa'],
    ['+', ['a+'], '5a+', '5aa'],
    ['?', ['ab?'], '5ab?', '5a'],
    ['$', ['a$'], '5a$', '5a'],
    ['|', ['a|b'], '5a|b', '5a'],
    ['(', ['a(b)'], '5a(b)', '5ab'],
    ['[', ['a[b]'], '5a[b]', '5ab'],
    ['{', ['a{2}'], '5a{2}', '5aa'],
  ])('escapes the regex metacharacter %s', (_meta, labels, literal, notLiteral) => {
    const re = buildQuantityRe(labels);
    expect(re.test(literal)).toBe(true);
    expect(re.test(notLiteral)).toBe(false);
  });

  // Capture group 1 is the FULL signed numeric literal — sign, mantissa and
  // exponent. That is what removes the need for a second regex (the old
  // unit-suffix `value.replace(...)` strip): the caller reads m[1]
  // instead of re-declaring the alternation to strip the suffix.
  it.each([
    ['-1.5e-2deg', '-1.5e-2'],
    ['.5mm', '.5'],
    ['80mm', '80'],
    ['1e+3mm', '1e+3'],
    ['-10mm', '-10'],
    ['7.8kg/m^3', '7.8'],
  ])('captures the whole signed literal of %s as group 1', (input, numeric) => {
    const m = buildQuantityRe([...BASE_UNIT_LABELS, 'kg/m^3']).exec(input);
    expect(m).not.toBeNull();
    expect(m![1]).toBe(numeric);
  });

  // Overlapping labels must resolve by the `$` anchor, not by branch order.
  it('resolves overlapping labels correctly under $-anchoring', () => {
    const re = buildQuantityRe(['kg/m^3', 'm^3', 'kg', 'g', 'mm', 'm']);
    expect(re.exec('7.8kg/m^3')![1]).toBe('7.8');
    expect(re.exec('5m^3')![1]).toBe('5');
    expect(re.exec('5kg')![1]).toBe('5');
    expect(re.exec('5g')![1]).toBe('5');
    expect(re.exec('80mm')![1]).toBe('80');
    expect(re.exec('80m')![1]).toBe('80');
  });

  it('is insensitive to the order labels are supplied in', () => {
    const forward = buildQuantityRe(['m', 'mm', 'kg', 'kg/m^3']);
    const reverse = buildQuantityRe(['kg/m^3', 'kg', 'mm', 'm']);
    expect(forward.source).toBe(reverse.source);
    for (const input of ['80mm', '80m', '5kg', '7.8kg/m^3']) {
      expect(forward.test(input)).toBe(true);
      expect(reverse.test(input)).toBe(true);
    }
  });

  describe('the legacy accept/reject table still holds against the baseline alphabet', () => {
    // Built lazily inside each case, not at describe-collection time, so a
    // missing/throwing builder fails these cases individually instead of
    // aborting collection for the whole file.
    const re = () => buildQuantityRe(BASE_UNIT_LABELS);

    it.each([
      ['80mm'], ['.5mm'], ['1e3mm'], ['1e+3mm'], ['1.5m'],
      ['100cm'], ['1rad'], ['90deg'], ['1.5e-2deg'], ['-10mm'],
      ['1e2mm'], ['-3.14rad'], ['0.5cm'], ['100deg'], ['.25deg'],
    ])('accepts %s', (input) => {
      expect(re().test(input)).toBe(true);
    });

    it.each([
      ['10xyz'],
      ['mm80'],
      // Leading '+' is rejected: the numeric part is `-?` (minus-only), so
      // '+10mm' fails even though the exponent group [eE][+-]? does accept
      // '+' ('1e+3mm' is valid). This matches the .ri grammar, which defines
      // only unary minus for number literals.
      ['+10mm'],
      // Both numeric branches require at least one digit, so a bare unit fails.
      ['mm'],
      ['deg'],
      // Whitespace between number and unit is rejected — the .ri grammar uses
      // token.immediate (tree-sitter-reify/grammar.js). The backend's
      // parse_value_string is more lenient; the frontend is deliberately not.
      ['5 mm'],
      ['5  mm'],
      ['5\tmm'],
      [' 5 mm '],
      ['Infinity'],
      ['-Infinity'],
      // Not in the baseline alphabet — widening is scoped to what the backend
      // actually advertises via the ladders.
      ['5mm^3'],
      ['5L'],
    ])('rejects %s', (input) => {
      expect(re().test(input)).toBe(false);
    });
  });

  // The overflow contract the existing suite documents: the regex has no
  // numeric range check, so `Number.isFinite(Number(m[1]))` in the caller is
  // load-bearing.
  it.each([
    ['1e999mm'],
    ['-1e999deg'],
    ['1e999m'],
  ])('matches the overflow string %s but group 1 converts to a non-finite number', (input) => {
    const m = buildQuantityRe(BASE_UNIT_LABELS).exec(input);
    expect(m).not.toBeNull();
    expect(Number.isFinite(Number(m![1]))).toBe(false);
  });
});

describe('acceptsBareNumber (task #5757)', () => {
  // THE RULE, not an enumeration: a cell needs a unit exactly when one CAN be
  // typed for it. The predicate takes a DIMENSION and a ladder MAP, never a
  // list of unit strings, so it cannot drift from the Rust-authored curated
  // table — the standing #5788 D6 prohibition documented on
  // `quantityUnitAlphabet` above.
  //
  // The backend is the authoritative gate (`parse_value_string_for_cell` in
  // `gui/src-tauri/src/engine.rs`, which refuses a Value::Int/Value::Real for a
  // dimension its `LADDER_COVERAGE` table records). This is its inline-feedback
  // mirror: it is what keeps the typed text on screen for correction instead of
  // discarding it and replacing it with an async error toast.

  const COVERED = [
    'Length',
    'Volume',
    'Density',
    'Area',
    'Angle',
    'Mass',
    'Pressure',
    'Force',
    'Energy',
    'Power',
  ];

  // Only MEMBERSHIP is consulted, so a one-rung stub per dimension is a
  // faithful stand-in for the real fetched map and keeps the fixture from
  // becoming an eleventh copy of the curated table.
  const LADDERS: UnitLadderMap = Object.fromEntries(
    COVERED.map((d) => [d, [{ label: 'x', si_scale: 1, is_default: true }]]),
  );

  it.each(COVERED.map((d) => [d]))(
    'disallows a bare number for the %s cell, which a curated ladder covers',
    (dimension) => {
      expect(acceptsBareNumber(dimension, LADDERS)).toBe(false);
    },
  );

  it.each([
    ['Torque'],
    ['Frequency'],
    ['MomentOfInertia'],
  ])(
    'ALLOWS a bare number for the %s cell, which NO curated ladder covers',
    (dimension) => {
      // EXPRESSIBILITY, not dimensionedness. The 1000x hazard this gate targets
      // presupposes that a unit COULD have been typed: `20` in a Volume cell is
      // ambiguous because `20mm^3` and `20L` were both available. Where the
      // picker offers nothing and the alphabet admits nothing, refusing the bare
      // number disambiguates nothing — it removes the cell's LAST accepted input
      // and bricks the row.
      //
      // Not a weakening, because the backend agrees: `dimension_requires_unit`
      // returns None for exactly these dimensions, so `set_parameter` accepts
      // the bare number as a canonical SI magnitude. Refusing here would refuse
      // input the engine would have taken.
      expect(acceptsBareNumber(dimension, LADDERS)).toBe(true);
    },
  );

  it.each([
    [undefined],
    [''],
  ])('allows a bare number when the dimension is %p', (dimension) => {
    // The backend emits an empty `dimension` string for non-scalar,
    // dimensionless AND composed-dimension cells. The first two are genuinely
    // unit-less and must keep taking bare numbers — every dimensionless slider
    // in the panel depends on it. The third used to be a KNOWN ASYMMETRY caught
    // only by the backend; since the coverage-conditional rule the backend does
    // not gate a composed dimension either, so the two ends now AGREE on it.
    expect(acceptsBareNumber(dimension, LADDERS)).toBe(true);
  });

  it.each([
    [undefined],
    [{}],
  ])(
    'keeps GATING the static floor with ladders %p — the get_unit_ladders fetch having failed',
    (ladders) => {
      // The ENGINE does not degrade on this path. Its `LADDER_COVERAGE` is built
      // in-process from the Rust-authored curated table and is always populated,
      // so `set_parameter` keeps refusing `80` in a Length cell whatever became
      // of `get_unit_ladders` (`App.tsx`'s one-shot fetch logs, toasts, and
      // leaves the map `{}`). Failing open here made the two ends disagree
      // exactly where the panel could not see the ladders, and in the harmful
      // direction: the panel accepted the bare number, `editSeed` seeded it, and
      // the engine then refused it behind the async toast that discards the
      // typed text — the failure task #5757 exists to remove, re-entered through
      // the degraded path.
      //
      // The floor is what the panel can still DESCRIBE with no ladder data:
      // `quantityUnitAlphabet` unions `BASE_UNIT_LABELS` in on every path, so a
      // Length cell gated here can still be satisfied inline with `80mm`, and
      // `editSeed` still seeds one via `editSeedUnitLabel`'s `?? val.unit`
      // fallback. Gating without that would be the brick this predicate avoids.
      for (const dimension of BASE_UNIT_DIMENSIONS) {
        expect(acceptsBareNumber(dimension, ladders)).toBe(false);
      }
    },
  );

  it.each([
    [undefined],
    [{}],
  ])('FAILS OPEN BELOW THE FLOOR with ladders %p', (ladders) => {
    // Below the floor the safe direction is the other one, and pinned as a
    // direction rather than an accident: with no ladder data the panel can
    // neither offer a unit for these dimensions nor accept one, so refusing the
    // bare number would remove the row's LAST accepted input while refusing
    // input the engine takes outright for the genuinely uncovered ones.
    const belowFloor = [...COVERED, 'Torque', 'NotADimension'].filter(
      (d) => !BASE_UNIT_DIMENSIONS.includes(d),
    );
    expect(belowFloor.length).toBeGreaterThan(0);
    for (const dimension of belowFloor) {
      expect(acceptsBareNumber(dimension, ladders)).toBe(true);
    }
  });

  it('gates the floor even when a PRESENT map omits it', () => {
    // The floor applies unconditionally — the same shape `quantityUnitAlphabet`
    // already has, since it unions `BASE_UNIT_LABELS` in on every path and not
    // just the ladder-less one. A complete map covers the floor anyway, so the
    // two forms differ only for a present-but-partial payload; matching the
    // alphabet there is what keeps the gate and the alphabet beside it reading
    // ONE rule.
    const partial: UnitLadderMap = {
      Volume: [{ label: 'L', si_scale: 1e-3, is_default: true }],
    };
    for (const dimension of BASE_UNIT_DIMENSIONS) {
      expect(acceptsBareNumber(dimension, partial)).toBe(false);
    }
    expect(acceptsBareNumber('Volume', partial)).toBe(false);
    expect(acceptsBareNumber('Torque', partial)).toBe(true);
  });

  it('keys on the floor plus map MEMBERSHIP — an unrecognised name is simply uncovered', () => {
    // Off the floor, the predicate reads whether the dimension has a ladder and
    // never what is in it, so it stays immune to the rung labels themselves
    // (#5788 D6). A name the curated table does not carry is uncovered by
    // definition.
    expect(acceptsBareNumber('Length', LADDERS)).toBe(false);
    expect(acceptsBareNumber('NotADimension', LADDERS)).toBe(true);
    expect(acceptsBareNumber(undefined, LADDERS)).toBe(true);
  });
});
