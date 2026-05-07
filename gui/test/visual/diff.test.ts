import { describe, it, expect } from "vitest";
import { compareImages, decideOutcome } from "./diff";
import type { ImageData } from "./diff";

// Helper: create a solid-colour 8×8 RGBA buffer
function solidBuffer(
  width: number,
  height: number,
  r: number,
  g: number,
  b: number,
  a = 255,
): Buffer {
  const buf = Buffer.alloc(width * height * 4);
  for (let i = 0; i < width * height; i++) {
    buf[i * 4 + 0] = r;
    buf[i * 4 + 1] = g;
    buf[i * 4 + 2] = b;
    buf[i * 4 + 3] = a;
  }
  return buf;
}

function makeImage(width: number, height: number, rgba: Buffer): ImageData {
  return { width, height, rgba };
}

describe("compareImages", () => {
  it("(a) identical 8×8 buffers → status:match, mismatchedPixels:0, mismatchPct:0", () => {
    const buf = solidBuffer(8, 8, 100, 150, 200);
    const baseline = makeImage(8, 8, buf);
    const captured = makeImage(8, 8, Buffer.from(buf)); // copy
    const result = compareImages(baseline, captured, {
      pixelThreshold: 0.1,
      mismatchPctLimit: 0.01,
    });
    expect(result.status).toBe("match");
    expect(result.mismatchedPixels).toBe(0);
    expect(result.mismatchPct).toBe(0);
  });

  it("(b) 8×8 baseline vs 8×16 captured → status:dimension-mismatch, no diffRgba", () => {
    const baseline = makeImage(8, 8, solidBuffer(8, 8, 255, 0, 0));
    const captured = makeImage(8, 16, solidBuffer(8, 16, 255, 0, 0));
    const result = compareImages(baseline, captured, {
      pixelThreshold: 0.1,
      mismatchPctLimit: 0.01,
    });
    expect(result.status).toBe("dimension-mismatch");
    expect(result.diffRgba).toBeNull();
  });

  it("(c) ~30% of pixels differ at threshold 0.1, mismatchPctLimit 0.01 → status:diff, mismatchPct > limit", () => {
    // baseline: all red, captured: ~30% white pixels (every ~3rd pixel)
    const width = 8;
    const height = 8;
    const baseBuf = solidBuffer(width, height, 255, 0, 0);
    const capBuf = solidBuffer(width, height, 255, 0, 0);
    // Change ~30% of pixels to white (pixels 0, 3, 6, 9, ... = every 3rd)
    let changed = 0;
    for (let i = 0; i < width * height; i += 3) {
      capBuf[i * 4 + 0] = 0;
      capBuf[i * 4 + 1] = 255;
      capBuf[i * 4 + 2] = 0;
      changed++;
    }
    const baseline = makeImage(width, height, baseBuf);
    const captured = makeImage(width, height, capBuf);
    const result = compareImages(baseline, captured, {
      pixelThreshold: 0.1,
      mismatchPctLimit: 0.01,
    });
    expect(result.status).toBe("diff");
    expect(result.mismatchPct).toBeGreaterThan(0.01);
  });

  it("(d) 32×32 buffers with exactly 1 pixel flipped (~0.001 mismatch) → status:match (within tolerance)", () => {
    // 32×32 = 1024 pixels total; 1 flipped pixel → mismatchPct ≈ 0.001 < 0.01 limit.
    // Exercises the within-tolerance-but-non-zero path, unlike (a) which uses identical buffers.
    const baseBuf = solidBuffer(32, 32, 200, 100, 50);
    const capBuf = Buffer.from(baseBuf);
    // Flip exactly one pixel (index 0) to a visually distinct colour so pixelmatch counts it.
    capBuf[0] = 50; // R was 200
    capBuf[1] = 200; // G was 100
    capBuf[2] = 100; // B was  50
    // alpha stays 255
    const baseline = makeImage(32, 32, baseBuf);
    const captured = makeImage(32, 32, capBuf);
    const result = compareImages(baseline, captured, {
      pixelThreshold: 0.1,
      mismatchPctLimit: 0.01,
    });
    expect(result.status).toBe("match");
    expect(result.mismatchedPixels).toBe(1);
    expect(result.mismatchPct).toBeLessThan(0.01);
  });
});

describe("decideOutcome", () => {
  const OPTS = { pixelThreshold: 0.1, mismatchPctLimit: 0.01, updateBaselines: false };

  it("(a) baseline=null, updateBaselines=false → baseline-created:missing", () => {
    const captured = makeImage(8, 8, solidBuffer(8, 8, 255, 0, 0));
    const outcome = decideOutcome(null, captured, OPTS);
    expect(outcome.kind).toBe("baseline-created");
    if (outcome.kind === "baseline-created") {
      expect(outcome.reason).toBe("missing");
    }
  });

  it("(b) baseline=non-null, updateBaselines=true → baseline-created:update-flag", () => {
    const buf = solidBuffer(8, 8, 200, 200, 200);
    const baseline = makeImage(8, 8, buf);
    const captured = makeImage(8, 8, Buffer.from(buf));
    const outcome = decideOutcome(baseline, captured, { ...OPTS, updateBaselines: true });
    expect(outcome.kind).toBe("baseline-created");
    if (outcome.kind === "baseline-created") {
      expect(outcome.reason).toBe("update-flag");
    }
  });

  it("(c) identical buffers → passed with mismatchedPixels:0, mismatchPct:0", () => {
    const buf = solidBuffer(8, 8, 50, 100, 150);
    const baseline = makeImage(8, 8, buf);
    const captured = makeImage(8, 8, Buffer.from(buf));
    const outcome = decideOutcome(baseline, captured, OPTS);
    expect(outcome.kind).toBe("passed");
    if (outcome.kind === "passed") {
      expect(outcome.mismatchedPixels).toBe(0);
      expect(outcome.mismatchPct).toBe(0);
    }
  });

  it("(d) ~30% differing pixels → failed:tolerance-exceeded with diffRgba Buffer", () => {
    const width = 8;
    const height = 8;
    const baseBuf = solidBuffer(width, height, 255, 0, 0);
    const capBuf = solidBuffer(width, height, 255, 0, 0);
    for (let i = 0; i < width * height; i += 3) {
      capBuf[i * 4 + 0] = 0;
      capBuf[i * 4 + 1] = 0;
      capBuf[i * 4 + 2] = 255;
    }
    const baseline = makeImage(width, height, baseBuf);
    const captured = makeImage(width, height, capBuf);
    const outcome = decideOutcome(baseline, captured, OPTS);
    expect(outcome.kind).toBe("failed");
    if (outcome.kind === "failed" && outcome.reason === "tolerance-exceeded") {
      expect(outcome.mismatchPct).toBeGreaterThan(0.01);
      expect(outcome.diffRgba).toBeInstanceOf(Buffer);
    }
  });

  it("(e) mismatched dimensions → failed:dimension-mismatch with all four dimension fields", () => {
    const baseline = makeImage(8, 8, solidBuffer(8, 8, 0, 255, 0));
    const captured = makeImage(16, 8, solidBuffer(16, 8, 0, 255, 0));
    const outcome = decideOutcome(baseline, captured, OPTS);
    expect(outcome.kind).toBe("failed");
    if (outcome.kind === "failed" && outcome.reason === "dimension-mismatch") {
      expect(outcome.baselineWidth).toBe(8);
      expect(outcome.baselineHeight).toBe(8);
      expect(outcome.capturedWidth).toBe(16);
      expect(outcome.capturedHeight).toBe(8);
    }
  });
});
