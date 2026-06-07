/**
 * Structural existence + cross-link guard for the debug-MCP recipe docs.
 *
 * Guards: docs/debug-mcp-recipe.md exists and is non-empty, and both
 * docs/skills/verify.md and docs/skills/review.md exist and contain the
 * 'debug-mcp-recipe.md' link (the anti-orphan cross-link contract).
 *
 * No prose/wording assertions — wording is not TDD-testable. The one-line
 * cross-link check is the 'referenced from the skills' contract (analogous
 * to the 'every fixture file exists on disk' test in assertions.test.ts).
 *
 * task-4305 E1 step-9 RED → step-10 GREEN
 */

import { describe, it, expect } from "vitest";
import * as path from "node:path";
import * as fs from "node:fs";
import { resolveRepoRoot } from "./paths.js";

const repoRoot = resolveRepoRoot(import.meta.url);

describe("debug-mcp-recipe docs (task-4305 E1)", () => {
  it("docs/debug-mcp-recipe.md exists and is non-empty", () => {
    const recipePath = path.join(repoRoot, "docs", "debug-mcp-recipe.md");
    expect(fs.existsSync(recipePath), `docs/debug-mcp-recipe.md not found at ${recipePath}`).toBe(true);
    const contents = fs.readFileSync(recipePath, "utf8");
    expect(contents.trim().length, "docs/debug-mcp-recipe.md must be non-empty").toBeGreaterThan(0);
  });

  it("docs/skills/verify.md exists and contains 'debug-mcp-recipe.md' link", () => {
    const verifyPath = path.join(repoRoot, "docs", "skills", "verify.md");
    expect(fs.existsSync(verifyPath), `docs/skills/verify.md not found at ${verifyPath}`).toBe(true);
    const contents = fs.readFileSync(verifyPath, "utf8");
    expect(
      contents.includes("debug-mcp-recipe.md"),
      "docs/skills/verify.md must contain the literal 'debug-mcp-recipe.md' cross-link",
    ).toBe(true);
  });

  it("docs/skills/review.md exists and contains 'debug-mcp-recipe.md' link", () => {
    const reviewPath = path.join(repoRoot, "docs", "skills", "review.md");
    expect(fs.existsSync(reviewPath), `docs/skills/review.md not found at ${reviewPath}`).toBe(true);
    const contents = fs.readFileSync(reviewPath, "utf8");
    expect(
      contents.includes("debug-mcp-recipe.md"),
      "docs/skills/review.md must contain the literal 'debug-mcp-recipe.md' cross-link",
    ).toBe(true);
  });
});
