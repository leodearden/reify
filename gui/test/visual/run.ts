/**
 * Visual regression integration harness.
 *
 * Usage:
 *   npm run test:visual                      # diff against baselines (exit 0 pass, 1 fail, 2 harness error)
 *   UPDATE_BASELINES=1 npm run test:visual   # force-rewrite all baselines
 *
 * CI integration: invoke this script in a CI job after the `cargo build` step,
 * once `.github/workflows/` exists in the repo (task TBD).
 *
 * Requires the reify-gui debug server to accept connections on 127.0.0.1:3939.
 * The server is started automatically by spawning scripts/run-gui-dev.sh.
 */

import * as fs from "node:fs";
import * as path from "node:path";
import * as child_process from "node:child_process";
import { PNG } from "pngjs";
import { parseRpcResponse, type RpcResult } from "./rpc.js";
import { decideOutcome } from "./diff.js";
import type { ImageData } from "./diff.js";
import { resolveRepoRoot, assertRepoRootStructure } from "./paths.js";
import { FIXTURES, VALUE_SCENARIOS, runValueScenario } from "./assertions.js";

// ─── Constants ────────────────────────────────────────────────────────────────

const PIXEL_THRESHOLD = 0.1;
const MISMATCH_PCT_LIMIT = 0.01;
const UPDATE_BASELINES = process.env.UPDATE_BASELINES === "1";
const DEBUG_URL = "http://127.0.0.1:3939/mcp";
const POLL_INTERVAL_MS = 1_000;
const SERVER_TIMEOUT_MS = 600_000;

const REPO_ROOT = resolveRepoRoot(import.meta.url);
assertRepoRootStructure(REPO_ROOT);
const SCREENSHOTS_DIR = path.join(REPO_ROOT, "gui", "test", "screenshots");

// ─── Scenario definitions ─────────────────────────────────────────────────────

interface Camera {
  position: [number, number, number];
  target: [number, number, number];
  up?: [number, number, number];
  zoom?: number;
}

interface Scenario {
  name: string;
  fixture: string;
  camera: Camera;
}

const SCENARIOS: Scenario[] = [
  {
    name: "m5_geometry_flange",
    fixture: "examples/m5_geometry_flange.ri",
    camera: {
      position: [0.15, 0.1, 0.15],
      target: [0, 0, 0],
    },
  },
];

// ─── RPC client ───────────────────────────────────────────────────────────────

async function rpc<T>(
  method: string,
  args: Record<string, unknown>,
): Promise<RpcResult<T>> {
  try {
    const response = await fetch(DEBUG_URL, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        jsonrpc: "2.0",
        id: 1,
        method: "tools/call",
        params: { name: method, arguments: args },
      }),
    });
    const envelope = await response.json();
    return parseRpcResponse<T>(envelope);
  } catch (err) {
    return { ok: false, error: `network: ${String(err)}` };
  }
}

// ─── Debug server polling ─────────────────────────────────────────────────────

async function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function waitForDebugServer(timeoutMs = SERVER_TIMEOUT_MS): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  let lastError = "timeout";
  while (Date.now() < deadline) {
    // Short-circuit early if spawn() itself failed (e.g. ENOENT) or the child exited
    // before the debug server became ready — avoids burning the full 600 s timeout.
    if (spawnFailed) {
      throw new Error("GUI process failed to start — see logs above");
    }
    if (guiProcess !== null && guiProcess.exitCode !== null) {
      throw new Error(
        `GUI process exited unexpectedly (code ${guiProcess.exitCode}) before debug server became ready`,
      );
    }
    const result = await rpc("health", {});
    if (result.ok) {
      console.log("[harness] debug server ready");
      return;
    }
    lastError = result.error;
    await sleep(POLL_INTERVAL_MS);
  }
  throw new Error(`Debug server did not become ready within ${timeoutMs}ms: ${lastError}`);
}

// ─── GUI process management ───────────────────────────────────────────────────

let guiProcess: child_process.ChildProcess | null = null;
/** Set to true by the error handler if spawn() itself fails (e.g. ENOENT). */
let spawnFailed = false;

function spawnGui(launchFixture: string): void {
  const fixturePath = path.join(REPO_ROOT, launchFixture);
  if (!fs.existsSync(fixturePath)) {
    throw new Error(`Fixture not found: ${fixturePath}`);
  }
  console.log(`[harness] spawning reify-gui with ${launchFixture}`);
  guiProcess = child_process.spawn(
    "scripts/run-gui-dev.sh",
    [fixturePath],
    {
      cwd: REPO_ROOT,
      stdio: ["ignore", "inherit", "inherit"],
      env: process.env,
    },
  );
  guiProcess.on("error", (err) => {
    console.error(`[harness] GUI process error: ${err.message}`);
    spawnFailed = true;
  });
}

async function reapGui(): Promise<void> {
  if (!guiProcess) return;
  const proc = guiProcess;
  guiProcess = null;
  proc.kill("SIGTERM");
  // Give 5 seconds for graceful shutdown, then SIGKILL
  await new Promise<void>((resolve) => {
    const timer = setTimeout(() => {
      proc.kill("SIGKILL");
      resolve();
    }, 5_000);
    proc.once("exit", () => {
      clearTimeout(timer);
      resolve();
    });
  });
}

// ─── PNG helpers ──────────────────────────────────────────────────────────────

function bufferToPng(rgba: Buffer, width: number, height: number): Buffer {
  const png = new PNG({ width, height });
  rgba.copy(png.data);
  return PNG.sync.write(png);
}

function pngToImageData(pngBuffer: Buffer): ImageData {
  const png = PNG.sync.read(pngBuffer);
  return { width: png.width, height: png.height, rgba: png.data as Buffer };
}

function writeScreenshot(filePath: string, rgba: Buffer, width: number, height: number): void {
  fs.mkdirSync(path.dirname(filePath), { recursive: true });
  fs.writeFileSync(filePath, bufferToPng(rgba, width, height));
}

// ─── Main ─────────────────────────────────────────────────────────────────────

type HarnessExitCode = 0 | 1 | 2;

async function main(): Promise<HarnessExitCode> {
  let anyFailed = false;
  // INVARIANT: SCENARIOS[0].fixture bootstraps the GUI process; subsequent scenarios call
  // open_file without relaunching. If scenarios are ever filtered or reordered (e.g. a
  // future --grep flag), move the launch fixture to a neutral empty .ri and rely entirely
  // on open_file for all scenarios — or keep this invariant and document it clearly.
  const firstFixture = SCENARIOS[0].fixture;

  // spawnGui() is the single source-of-truth for the fixture existence check; its throw
  // is caught below along with any other harness-level errors.
  try {
    spawnGui(firstFixture);
    await waitForDebugServer();

    for (const scenario of SCENARIOS) {
      console.log(`\n[harness] scenario: ${scenario.name}`);

      // Open the scenario fixture
      const fixturePath = path.join(REPO_ROOT, scenario.fixture);
      const openResult = await rpc<unknown>("open_file", { path: fixturePath });
      if (!openResult.ok) {
        console.error(`  FAIL open_file: ${openResult.error}`);
        anyFailed = true;
        continue;
      }

      // Enable test mode (deterministic rendering)
      const testModeResult = await rpc<unknown>("set_test_mode", { enabled: true });
      if (!testModeResult.ok) {
        console.error(`  FAIL set_test_mode: ${testModeResult.error}`);
        anyFailed = true;
        continue;
      }

      // Set camera
      const cameraArgs: Record<string, unknown> = {
        position: scenario.camera.position,
        target: scenario.camera.target,
      };
      if (scenario.camera.up !== undefined) cameraArgs.up = scenario.camera.up;
      if (scenario.camera.zoom !== undefined) cameraArgs.zoom = scenario.camera.zoom;

      const cameraResult = await rpc<unknown>("set_camera", cameraArgs);
      if (!cameraResult.ok) {
        console.error(`  FAIL set_camera: ${cameraResult.error}`);
        anyFailed = true;
        continue;
      }

      // Wait for the renderer to settle
      const idleResult = await rpc<unknown>("wait_for_idle", { timeout_ms: 30_000 });
      if (!idleResult.ok) {
        console.error(`  FAIL wait_for_idle: ${idleResult.error}`);
        anyFailed = true;
        continue;
      }

      // Capture screenshot
      const shotResult = await rpc<{ data: string }>("screenshot", {});
      if (!shotResult.ok) {
        console.error(`  FAIL screenshot: ${shotResult.error}`);
        anyFailed = true;
        continue;
      }

      // Decode captured PNG from base64
      const capturedPngBuffer = Buffer.from(shotResult.value.data, "base64");
      const capturedImage = pngToImageData(capturedPngBuffer);

      // Read baseline if it exists
      const baselinePath = path.join(SCREENSHOTS_DIR, `${scenario.name}.png`);
      let baselineImage: ImageData | null = null;
      if (fs.existsSync(baselinePath)) {
        try {
          const baselinePng = fs.readFileSync(baselinePath);
          baselineImage = pngToImageData(baselinePng);
        } catch (err) {
          console.error(`  FAIL reading baseline: ${String(err)}`);
          anyFailed = true;
          continue;
        }
      }

      // Decide outcome
      const outcome = decideOutcome(baselineImage, capturedImage, {
        pixelThreshold: PIXEL_THRESHOLD,
        mismatchPctLimit: MISMATCH_PCT_LIMIT,
        updateBaselines: UPDATE_BASELINES,
      });

      switch (outcome.kind) {
        case "baseline-created":
          writeScreenshot(baselinePath, capturedImage.rgba, capturedImage.width, capturedImage.height);
          console.log(`  BASE ${scenario.name} (reason: ${outcome.reason})`);
          break;

        case "passed":
          console.log(
            `  PASS ${scenario.name} (${outcome.mismatchedPixels} px, ${(outcome.mismatchPct * 100).toFixed(3)}%)`,
          );
          break;

        case "failed": {
          anyFailed = true;
          // Write actual screenshot
          const actualPath = path.join(SCREENSHOTS_DIR, `${scenario.name}.actual.png`);
          writeScreenshot(actualPath, capturedImage.rgba, capturedImage.width, capturedImage.height);

          if (outcome.reason === "tolerance-exceeded") {
            const diffPath = path.join(SCREENSHOTS_DIR, `${scenario.name}.diff.png`);
            writeScreenshot(diffPath, outcome.diffRgba, capturedImage.width, capturedImage.height);
            console.error(
              `  FAIL ${scenario.name} — tolerance exceeded: ${(outcome.mismatchPct * 100).toFixed(3)}% mismatched pixels`,
            );
            console.error(`       actual: ${actualPath}`);
            console.error(`       diff:   ${diffPath}`);
          } else {
            // dimension-mismatch
            console.error(
              `  FAIL ${scenario.name} — dimension mismatch: baseline ${outcome.baselineWidth}×${outcome.baselineHeight} vs captured ${outcome.capturedWidth}×${outcome.capturedHeight}`,
            );
            console.error(`       actual: ${actualPath}`);
          }
          break;
        }
      }
    }
  } catch (err) {
    console.error(`[harness] FATAL: ${String(err)}`);
    return 2;
  }

  return anyFailed ? 1 : 0;
}

// ─── Value-assertion mode ─────────────────────────────────────────────────────

/**
 * Run all VALUE_SCENARIOS against the live reify-gui debug server.
 *
 * Boots the GUI with the small_cube fixture (same spawnGui/waitForDebugServer/
 * reapGui lifecycle as visual mode, no new boot/teardown code). For each
 * scenario, openFixture resolves the repo-relative path to absolute, calls
 * open_file + wait_for_idle, then callTool calls the named tool. Results are
 * logged per-scenario; exit code 0 = all passed, 1 = any failed, 2 = fatal.
 */
async function runValueScenarios(): Promise<HarnessExitCode> {
  let anyFailed = false;

  try {
    spawnGui(FIXTURES.small_cube);
    await waitForDebugServer();

    async function openFixture(repoRelPath: string): Promise<RpcResult<unknown>> {
      const absPath = path.join(REPO_ROOT, repoRelPath);
      const openResult = await rpc<unknown>("open_file", { path: absPath });
      if (!openResult.ok) return openResult;
      const idleResult = await rpc<unknown>("wait_for_idle", { timeout_ms: 30_000 });
      if (!idleResult.ok) return idleResult;
      return { ok: true, value: idleResult.value };
    }

    async function callTool(tool: string, args: Record<string, unknown>): Promise<RpcResult<unknown>> {
      return rpc<unknown>(tool, args);
    }

    for (const scenario of VALUE_SCENARIOS) {
      console.log(`\n[harness:value] scenario: ${scenario.name}`);
      const result = await runValueScenario({ openFixture, callTool }, scenario);
      if (result.passed) {
        console.log(`  PASS ${result.name}`);
      } else {
        anyFailed = true;
        console.error(`  FAIL ${result.name}`);
        for (const msg of result.failures) {
          console.error(`       ${msg}`);
        }
      }
    }
  } catch (err) {
    console.error(`[harness:value] FATAL: ${String(err)}`);
    return 2;
  }

  return anyFailed ? 1 : 0;
}

// ─── Entry point ──────────────────────────────────────────────────────────────

function shutdown(exitCode: number): void {
  reapGui().finally(() => {
    process.exit(exitCode);
  });
}

// Ensure the spawned reify-gui + vite tree is reaped on Ctrl-C / CI cancellation.
// Without these handlers the child processes leak and leave ports 1420/3939 bound.
process.on("SIGINT", () => shutdown(130));
process.on("SIGTERM", () => shutdown(143));

const MODE = process.argv.includes("value") ? "value" : "visual";
const harness = MODE === "value" ? runValueScenarios() : main();

harness
  .then((code) => {
    reapGui().finally(() => {
      process.exit(code);
    });
  })
  .catch((err) => {
    console.error(`[harness] unhandled error: ${String(err)}`);
    reapGui().finally(() => {
      process.exit(2);
    });
  });
