/**
 * Injectable-rpc orchestration seam for the visual-regression harness.
 *
 * Pure module — no I/O, no top-level side effects. Drives a single SCENARIOS
 * entry through the fixed per-scenario RPC sequence (open_file -> set_test_mode
 * -> set_camera -> wait_for_idle -> optional set_fea_case block -> optional
 * feaChannel block -> optional feaView block) up to screenshot-ready state.
 * The live rpc() HTTP client, GUI process spawn/teardown, and the
 * screenshot/PNG-decode/pixel-diff/baseline I/O all stay in run.ts.
 *
 * Mirrors the assertions.ts split: runValueScenario takes injected deps and
 * is headlessly unit-tested in assertions.test.ts, while its I/O wrapper
 * (runValueScenarios) lives in run.ts. This module is runScenarioSteps'
 * analogue for the SCENARIOS (visual) orchestration, headlessly tested in
 * orchestrate.test.ts.
 */

import * as path from "node:path";
import type { RpcResult } from "./rpc.js";
import { feaChannelActions, type Scenario } from "./scenarios.js";

/** Injected RPC call — same shape as run.ts's module-level rpc(). */
export type RpcFn = <T>(method: string, args: Record<string, unknown>) => Promise<RpcResult<T>>;

/** Injected logger — run.ts passes `(m) => console.error(m)`. */
export type LogFn = (message: string) => void;

/** Injected I/O dependencies for runScenarioSteps. */
export type ScenarioRunDeps = {
  rpc: RpcFn;
  log: LogFn;
};

/** Outcome of driving a scenario through runScenarioSteps. */
export type ScenarioStepsOutcome =
  | { ok: true }
  | { ok: false; failedLabel: string; error: string };

/**
 * Drive a single scenario through the common RPC sequence (open_file,
 * set_test_mode, set_camera, wait_for_idle) up to screenshot-ready state.
 *
 * On any step's ok:false result, logs "  FAIL <label>: <error>" via
 * deps.log and returns {ok:false, failedLabel, error} immediately — later
 * steps are never invoked. Log-and-continue across *scenarios* is the
 * caller's responsibility (mirrors run.ts main()'s per-scenario `continue`).
 */
export async function runScenarioSteps(
  deps: ScenarioRunDeps,
  scenario: Scenario,
  repoRoot: string,
): Promise<ScenarioStepsOutcome> {
  async function step(
    label: string,
    method: string,
    args: Record<string, unknown>,
  ): Promise<ScenarioStepsOutcome | null> {
    const result = await deps.rpc<unknown>(method, args);
    if (!result.ok) {
      deps.log(`  FAIL ${label}: ${result.error}`);
      return { ok: false, failedLabel: label, error: result.error };
    }
    return null;
  }

  const fixturePath = path.join(repoRoot, scenario.fixture);

  let f = await step("open_file", "open_file", { path: fixturePath });
  if (f) return f;

  f = await step("set_test_mode", "set_test_mode", { enabled: true });
  if (f) return f;

  const cameraArgs: Record<string, unknown> = {
    position: scenario.camera.position,
    target: scenario.camera.target,
  };
  if (scenario.camera.up !== undefined) cameraArgs.up = scenario.camera.up;
  if (scenario.camera.zoom !== undefined) cameraArgs.zoom = scenario.camera.zoom;
  f = await step("set_camera", "set_camera", cameraArgs);
  if (f) return f;

  f = await step("wait_for_idle (stuck renderer/engine?)", "wait_for_idle", { timeout_ms: 30_000 });
  if (f) return f;

  // Select the active FEA load case (task 3026: multi-load-case case-picker).
  // When scenario.feaCase is set, call set_fea_case then wait for idle again
  // so the re-sourced contour is fully rendered before the caller screenshots.
  if (scenario.feaCase !== undefined) {
    f = await step(`set_fea_case(${scenario.feaCase})`, "set_fea_case", { case: scenario.feaCase });
    if (f) return f;

    f = await step("wait_for_idle after set_fea_case", "wait_for_idle", { timeout_ms: 30_000 });
    if (f) return f;
  }

  return { ok: true };
}
