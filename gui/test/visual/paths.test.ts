import { describe, it, expect } from "vitest";
import * as path from "node:path";
import * as fs from "node:fs";
import { resolveRepoRoot, assertRepoRootStructure } from "./paths.js";

describe("resolveRepoRoot", () => {
  it("(a) returns /home/leo/src/reify for a file in gui/test/visual/", () => {
    const result = resolveRepoRoot("file:///home/leo/src/reify/gui/test/visual/run.ts");
    expect(result).toBe("/home/leo/src/reify");
  });

  it("(b) handles a different prefix — same 3-level ascent", () => {
    const result = resolveRepoRoot("file:///alt/path/gui/test/visual/x.ts");
    expect(result).toBe("/alt/path");
  });

  it("(c) resolveRepoRoot(import.meta.url) resolves to a directory containing gui/package.json", () => {
    const repoRoot = resolveRepoRoot(import.meta.url);
    const guiPkg = path.join(repoRoot, "gui", "package.json");
    expect(fs.existsSync(guiPkg)).toBe(true);
  });
});

describe("assertRepoRootStructure", () => {
  it("(d) does not throw for the real repo root", () => {
    const repoRoot = resolveRepoRoot(import.meta.url);
    expect(() => assertRepoRootStructure(repoRoot)).not.toThrow();
  });

  it("(e) throws with a message mentioning REPO_ROOT and gui/package.json for a bogus path", () => {
    expect(() => assertRepoRootStructure("/tmp/definitely-not-a-repo-root-xyz")).toThrow(
      expect.objectContaining({
        message: expect.stringContaining("REPO_ROOT"),
      }),
    );
    expect(() => assertRepoRootStructure("/tmp/definitely-not-a-repo-root-xyz")).toThrow(
      expect.objectContaining({
        message: expect.stringContaining("gui/package.json"),
      }),
    );
  });
});
