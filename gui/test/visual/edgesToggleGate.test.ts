/**
 * The CI half of the edges-in-black toggle smoke (task 9731): every decision the
 * live driver `./smoke_edges_toggle_e2e.mjs` makes, as pure data.
 *
 * Failure records are asserted on their stable `code`, never on message text.
 */
import { describe, it, expect } from "vitest";
import {
  EDGES_GATE_CODES,
  MIN_CHANGED_PIXELS,
  countChangedPixels,
  gradeEdgesToggle,
  formatFailures,
} from "./edgesToggleGate.mjs";

interface Shot {
  width: number;
  height: number;
  data: Uint8Array;
}

/** A uniform mid-grey RGBA image. */
function shot(width: number, height: number): Shot {
  return { width, height, data: new Uint8Array(width * height * 4).fill(128) };
}

/** A copy of `base` with its first `n` pixels altered in one channel each (cycling R, G, B, A). */
function withChangedPixels(base: Shot, n: number): Shot {
  const data = base.data.slice();
  for (let p = 0; p < n; p++) data[p * 4 + (p % 4)] = 0;
  return { ...base, data };
}

const codes = (failures: Array<{ code: string }>) => failures.map((f) => f.code);

const OFF = shot(64, 64);
const ON = withChangedPixels(OFF, MIN_CHANGED_PIXELS);
const PASSING = { edgesAfterOn: true, edgesAfterOff: false, offShot: OFF, onShot: ON };

describe("countChangedPixels", () => {
  it("counts pixels that differ in any RGBA channel", () => {
    expect(countChangedPixels(OFF, OFF)).toBe(0);
    expect(countChangedPixels(OFF, withChangedPixels(OFF, 7))).toBe(7);
  });

  it("returns null when the sizes differ", () => {
    expect(countChangedPixels(OFF, shot(64, 32))).toBeNull();
    expect(countChangedPixels(OFF, shot(32, 64))).toBeNull();
  });
});

describe("gradeEdgesToggle", () => {
  it("passes when the toggle flipped on then off and the shots differ enough", () => {
    expect(gradeEdgesToggle(PASSING)).toEqual([]);
  });

  it("fails identical shots as too similar", () => {
    const failures = gradeEdgesToggle({ ...PASSING, onShot: OFF });
    expect(codes(failures)).toEqual([EDGES_GATE_CODES.SHOTS_TOO_SIMILAR]);
    expect(failures[0]).toMatchObject({ changedPixels: 0 });
  });

  it("fails one pixel short of the threshold as too similar", () => {
    const failures = gradeEdgesToggle({ ...PASSING, onShot: withChangedPixels(OFF, MIN_CHANGED_PIXELS - 1) });
    expect(codes(failures)).toEqual([EDGES_GATE_CODES.SHOTS_TOO_SIMILAR]);
    expect(failures[0]).toMatchObject({ changedPixels: MIN_CHANGED_PIXELS - 1 });
  });

  it("fails a size mismatch, reporting both sizes", () => {
    const failures = gradeEdgesToggle({ ...PASSING, onShot: shot(32, 64) });
    expect(codes(failures)).toEqual([EDGES_GATE_CODES.SHOT_SIZE_MISMATCH]);
    expect(failures[0]).toMatchObject({
      offSize: { width: 64, height: 64 },
      onSize: { width: 32, height: 64 },
    });
  });

  it.each([false, undefined])("fails when edges did not read true after toggling on (%s)", (observed) => {
    const failures = gradeEdgesToggle({ ...PASSING, edgesAfterOn: observed });
    expect(codes(failures)).toEqual([EDGES_GATE_CODES.EDGES_NOT_ON]);
    expect(failures[0]).toMatchObject({ observed });
  });

  it("fails when edges did not read false after toggling back off", () => {
    const failures = gradeEdgesToggle({ ...PASSING, edgesAfterOff: true });
    expect(codes(failures)).toEqual([EDGES_GATE_CODES.EDGES_NOT_OFF]);
    expect(failures[0]).toMatchObject({ observed: true });
  });

  it("reports every failure at once", () => {
    const failures = gradeEdgesToggle({ edgesAfterOn: false, edgesAfterOff: true, offShot: OFF, onShot: OFF });
    expect(codes(failures).sort()).toEqual(
      [
        EDGES_GATE_CODES.EDGES_NOT_ON,
        EDGES_GATE_CODES.EDGES_NOT_OFF,
        EDGES_GATE_CODES.SHOTS_TOO_SIMILAR,
      ].sort(),
    );
  });
});

describe("formatFailures", () => {
  it("renders one line per failure, each naming its code", () => {
    const failures = gradeEdgesToggle({ edgesAfterOn: false, edgesAfterOff: true, offShot: OFF, onShot: OFF });
    const lines = formatFailures(failures).split("\n");
    expect(lines).toHaveLength(failures.length);
    failures.forEach((f, i) => expect(lines[i]).toContain(f.code));
  });
});
