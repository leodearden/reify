import { describe, it, expect } from "vitest";
import type { RpcResult } from "./rpc.js";
import { SCENARIOS, type Scenario } from "./scenarios.js";
import {
  runScenarioSteps,
  type RpcFn,
  type ScenarioRunDeps,
  type ScenarioStepsOutcome,
} from "./orchestrate.js";

const REPO_ROOT = "/repo";

type Call = { method: string; args: Record<string, unknown> };

/**
 * A scripted result for one method: either a single RpcResult applied to
 * every call of that method, or an array of RpcResults consumed one-per-call
 * in invocation order (index 0 = the method's first call, index 1 = its
 * second, etc). Once an array is exhausted, later calls of that method
 * default to {ok:true, value:null}, same as an unscripted method.
 *
 * The array form is what makes a *specific occurrence* of a repeated method
 * independently scriptable — e.g. failing only the trailing "wait_for_idle
 * after feaChannelActions" call without also failing the common-prefix
 * wait_for_idle, or failing only the second click_element (warp preset)
 * without touching the first (show-deformed toggle). A bare RpcResult can't
 * distinguish those since both calls share the same method name.
 */
type ScriptedResult = RpcResult<unknown> | RpcResult<unknown>[];

/**
 * Build a recording fake rpc + log pair for runScenarioSteps tests.
 *
 * `scripted` maps a method name to the RpcResult(s) it should return. A bare
 * RpcResult applies to every call of that method; an array is consumed in
 * order, one element per call (see ScriptedResult above). Any call beyond a
 * scripted array's length, or any unscripted method, defaults to
 * {ok:true, value:null}. Every call (scripted or not) is pushed onto `calls`,
 * in invocation order, before the scripted result is returned — so a
 * scripted failure still shows up in `calls` (the call itself was made; it
 * just failed).
 */
function makeFakeDeps(scripted: Partial<Record<string, ScriptedResult>> = {}) {
  const calls: Call[] = [];
  const logs: string[] = [];
  const callCounts: Record<string, number> = {};
  const rpc: RpcFn = async <T>(method: string, args: Record<string, unknown>): Promise<RpcResult<T>> => {
    calls.push({ method, args });
    const scriptedForMethod = scripted[method];
    let result: RpcResult<unknown> | undefined;
    if (Array.isArray(scriptedForMethod)) {
      const callIndex = callCounts[method] ?? 0;
      callCounts[method] = callIndex + 1;
      result = scriptedForMethod[callIndex];
    } else {
      result = scriptedForMethod;
    }
    return (result ?? { ok: true, value: null }) as RpcResult<T>;
  };
  const log = (m: string) => logs.push(m);
  const deps: ScenarioRunDeps = { rpc, log };
  return { deps, calls, logs };
}

// ─── Synthetic scenario fixtures ───────────────────────────────────────────
//
// Pure-orchestration assertions below drive synthetic Scenario literals, not
// the live SCENARIOS catalogue, so an unrelated camera/fixture/name tweak in
// scenarios.ts can't break these tests — only the m5_geometry_flange smoke
// test keeps a binding to a real catalogue entry.

const SYNTHETIC_PLAIN: Scenario = {
  name: "synthetic_plain",
  fixture: "examples/synthetic_plain.ri",
  camera: { position: [1, 2, 3], target: [4, 5, 6] },
};

const SYNTHETIC_CAMERA_UP_ZOOM: Scenario = {
  name: "synthetic_camera_up_zoom",
  fixture: "examples/synthetic_camera_up_zoom.ri",
  camera: {
    position: [1, 2, 3],
    target: [0, 0, 0],
    up: [0, 1, 0],
    zoom: 2.5,
  },
};

const SYNTHETIC_FEA_CASE: Scenario = {
  name: "synthetic_fea_case",
  fixture: "examples/synthetic_fea_case.ri",
  camera: { position: [1, 2, 3], target: [0, 0, 0] },
  feaCase: "synthetic_case",
};

const SYNTHETIC_FEA_CHANNEL: Scenario = {
  name: "synthetic_fea_channel",
  fixture: "examples/synthetic_fea_channel.ri",
  camera: { position: [1, 2, 3], target: [0, 0, 0] },
  feaChannel: "synthetic_channel",
};

const SYNTHETIC_FEA_VIEW_DEFORMED: Scenario = {
  name: "synthetic_fea_view_deformed",
  fixture: "examples/synthetic_fea_view.ri",
  camera: { position: [1, 2, 3], target: [0, 0, 0] },
  feaView: { deformed: true, warp: 10 },
};

const SYNTHETIC_FEA_VIEW_CONTOUR: Scenario = {
  name: "synthetic_fea_view_contour",
  fixture: "examples/synthetic_fea_view.ri",
  camera: { position: [1, 2, 3], target: [0, 0, 0] },
  feaView: { deformed: false },
};

// Combines feaChannel + feaView so wait_for_selector is called twice (once
// for the channel dropdown, once for the warp preset) — used only to cover
// the "second occurrence of a repeated method" failure cases below. Nothing
// in the real SCENARIOS catalogue currently combines the two, but the
// orchestration code doesn't treat them as mutually exclusive (unlike
// feaChannel/feaCase), so this is a legitimate synthetic case.
const SYNTHETIC_FEA_CHANNEL_AND_VIEW: Scenario = {
  name: "synthetic_fea_channel_and_view",
  fixture: "examples/synthetic_fea_channel_and_view.ri",
  camera: { position: [1, 2, 3], target: [0, 0, 0] },
  feaChannel: "synthetic_channel",
  feaView: { deformed: true, warp: 10 },
};

describe("runScenarioSteps — common prefix (open_file, set_test_mode, set_camera, wait_for_idle)", () => {
  it("(smoke) a real SCENARIOS entry (m5_geometry_flange) emits the 4-call common prefix and returns ok:true", async () => {
    const realScenario = SCENARIOS.find((s) => s.name === "m5_geometry_flange")!;
    const { deps, calls } = makeFakeDeps();
    const outcome: ScenarioStepsOutcome = await runScenarioSteps(deps, realScenario, REPO_ROOT);
    expect(calls).toEqual([
      { method: "open_file", args: { path: "/repo/examples/m5_geometry_flange.ri" } },
      { method: "set_test_mode", args: { enabled: true } },
      { method: "set_camera", args: { position: [0.15, 0.1, 0.15], target: [0, 0, 0] } },
      { method: "wait_for_idle", args: { timeout_ms: 30_000 } },
    ]);
    expect(outcome).toEqual({ ok: true });
  });

  it("(a) synthetic plain scenario emits exactly the 4-call common prefix and returns ok:true", async () => {
    const { deps, calls } = makeFakeDeps();
    const outcome = await runScenarioSteps(deps, SYNTHETIC_PLAIN, REPO_ROOT);
    expect(calls).toEqual([
      { method: "open_file", args: { path: "/repo/examples/synthetic_plain.ri" } },
      { method: "set_test_mode", args: { enabled: true } },
      { method: "set_camera", args: { position: [1, 2, 3], target: [4, 5, 6] } },
      { method: "wait_for_idle", args: { timeout_ms: 30_000 } },
    ]);
    expect(outcome).toEqual({ ok: true });
  });

  it("(b) set_camera args carry no up/zoom keys when the scenario camera omits them", async () => {
    const { deps, calls } = makeFakeDeps();
    await runScenarioSteps(deps, SYNTHETIC_PLAIN, REPO_ROOT);
    const cameraCall = calls.find((c) => c.method === "set_camera")!;
    expect(Object.keys(cameraCall.args)).toEqual(["position", "target"]);
  });

  it("(c) set_camera args include up and zoom when the scenario camera sets them", async () => {
    const { deps, calls } = makeFakeDeps();
    const outcome = await runScenarioSteps(deps, SYNTHETIC_CAMERA_UP_ZOOM, REPO_ROOT);
    const cameraCall = calls.find((c) => c.method === "set_camera")!;
    expect(cameraCall.args).toEqual({
      position: [1, 2, 3],
      target: [0, 0, 0],
      up: [0, 1, 0],
      zoom: 2.5,
    });
    expect(outcome).toEqual({ ok: true });
  });

  it("(d) open_file failure short-circuits: ok:false/failedLabel/error, FAIL log, no later prefix calls", async () => {
    const { deps, calls, logs } = makeFakeDeps({
      open_file: { ok: false, error: "boom" },
    });
    const outcome = await runScenarioSteps(deps, SYNTHETIC_PLAIN, REPO_ROOT);
    expect(outcome).toEqual({ ok: false, failedLabel: "open_file", error: "boom" });
    expect(logs.some((l) => l.includes("FAIL open_file"))).toBe(true);
    expect(calls).toEqual([{ method: "open_file", args: { path: "/repo/examples/synthetic_plain.ri" } }]);
  });

  it("(e) mid-prefix failure (set_camera): wait_for_idle is never called and failedLabel is set_camera", async () => {
    const { deps, calls } = makeFakeDeps({
      set_camera: { ok: false, error: "camera exploded" },
    });
    const outcome = await runScenarioSteps(deps, SYNTHETIC_PLAIN, REPO_ROOT);
    expect(outcome.ok).toBe(false);
    if (!outcome.ok) {
      expect(outcome.failedLabel).toBe("set_camera");
    }
    expect(calls.some((c) => c.method === "wait_for_idle")).toBe(false);
  });

  it("(f) set_test_mode failure: failedLabel is set_test_mode, no later prefix calls", async () => {
    const { deps, calls, logs } = makeFakeDeps({
      set_test_mode: { ok: false, error: "mode rejected" },
    });
    const outcome = await runScenarioSteps(deps, SYNTHETIC_PLAIN, REPO_ROOT);
    expect(outcome).toEqual({ ok: false, failedLabel: "set_test_mode", error: "mode rejected" });
    expect(logs.some((l) => l.includes("FAIL set_test_mode"))).toBe(true);
    expect(calls).toEqual([
      { method: "open_file", args: { path: "/repo/examples/synthetic_plain.ri" } },
      { method: "set_test_mode", args: { enabled: true } },
    ]);
  });

  it("(g) common-prefix wait_for_idle failure: failedLabel carries the stuck-renderer wording, nothing recorded after it", async () => {
    const { deps, calls, logs } = makeFakeDeps({
      wait_for_idle: { ok: false, error: "stuck" },
    });
    const outcome = await runScenarioSteps(deps, SYNTHETIC_PLAIN, REPO_ROOT);
    expect(outcome).toEqual({
      ok: false,
      failedLabel: "wait_for_idle (stuck renderer/engine?)",
      error: "stuck",
    });
    expect(logs.some((l) => l.includes("FAIL wait_for_idle (stuck renderer/engine?)"))).toBe(true);
    expect(calls).toHaveLength(4);
  });
});

describe("runScenarioSteps — set_fea_case block (task 3026)", () => {
  it("(a) appends set_fea_case then wait_for_idle after the common prefix, outcome ok", async () => {
    const { deps, calls } = makeFakeDeps();
    const outcome = await runScenarioSteps(deps, SYNTHETIC_FEA_CASE, REPO_ROOT);
    expect(calls).toHaveLength(6);
    expect(calls.slice(4)).toEqual([
      { method: "set_fea_case", args: { case: "synthetic_case" } },
      { method: "wait_for_idle", args: { timeout_ms: 30_000 } },
    ]);
    expect(outcome).toEqual({ ok: true });
  });

  it("(b) set_fea_case failure: failedLabel/log name set_fea_case(synthetic_case), trailing wait_for_idle NOT recorded", async () => {
    const { deps, calls, logs } = makeFakeDeps({
      set_fea_case: { ok: false, error: "no case" },
    });
    const outcome = await runScenarioSteps(deps, SYNTHETIC_FEA_CASE, REPO_ROOT);
    expect(outcome).toEqual({ ok: false, failedLabel: "set_fea_case(synthetic_case)", error: "no case" });
    expect(logs.some((l) => l.includes("FAIL set_fea_case(synthetic_case)"))).toBe(true);
    // 4 common-prefix calls + the failing set_fea_case call = 5; no trailing wait_for_idle.
    expect(calls).toHaveLength(5);
    expect(calls[4]).toEqual({ method: "set_fea_case", args: { case: "synthetic_case" } });
  });

  it("(c) trailing wait_for_idle-after-set_fea_case failure: failedLabel carries the trailing label", async () => {
    const { deps, calls, logs } = makeFakeDeps({
      // index 0 = the common-prefix wait_for_idle (succeeds); index 1 = the
      // trailing "after set_fea_case" wait_for_idle (fails).
      wait_for_idle: [{ ok: true, value: null }, { ok: false, error: "stuck after case switch" }],
    });
    const outcome = await runScenarioSteps(deps, SYNTHETIC_FEA_CASE, REPO_ROOT);
    expect(outcome).toEqual({
      ok: false,
      failedLabel: "wait_for_idle after set_fea_case",
      error: "stuck after case switch",
    });
    expect(logs.some((l) => l.includes("FAIL wait_for_idle after set_fea_case"))).toBe(true);
    expect(calls).toHaveLength(6);
  });
});

describe("runScenarioSteps — feaChannel block (task 4906)", () => {
  it("(a) appends wait_for_selector, set_fea_channel, wait_for_idle after the common prefix, outcome ok", async () => {
    const { deps, calls } = makeFakeDeps();
    const outcome = await runScenarioSteps(deps, SYNTHETIC_FEA_CHANNEL, REPO_ROOT);
    expect(calls).toHaveLength(7);
    expect(calls.slice(4)).toEqual([
      { method: "wait_for_selector", args: { testId: "fea-mode-channel-select", timeout_ms: 30_000 } },
      { method: "set_fea_channel", args: { channel: "synthetic_channel" } },
      { method: "wait_for_idle", args: { timeout_ms: 30_000 } },
    ]);
    expect(outcome).toEqual({ ok: true });
  });

  it("(b) wait_for_selector(fea-mode-channel-select) failure: set_fea_channel NOT recorded, failedLabel matches", async () => {
    const { deps, calls } = makeFakeDeps({
      wait_for_selector: { ok: false, error: "selector timeout" },
    });
    const outcome = await runScenarioSteps(deps, SYNTHETIC_FEA_CHANNEL, REPO_ROOT);
    expect(outcome.ok).toBe(false);
    if (!outcome.ok) {
      expect(outcome.failedLabel).toBe("wait_for_selector(fea-mode-channel-select)");
    }
    expect(calls.some((c) => c.method === "set_fea_channel")).toBe(false);
  });

  it("(c) set_fea_channel failure: trailing wait_for_idle NOT recorded, failedLabel/log name set_fea_channel(synthetic_channel)", async () => {
    const { deps, calls, logs } = makeFakeDeps({
      set_fea_channel: { ok: false, error: "bad channel" },
    });
    const outcome = await runScenarioSteps(deps, SYNTHETIC_FEA_CHANNEL, REPO_ROOT);
    expect(outcome).toEqual({ ok: false, failedLabel: "set_fea_channel(synthetic_channel)", error: "bad channel" });
    expect(logs.some((l) => l.includes("FAIL set_fea_channel(synthetic_channel)"))).toBe(true);
    // 4 common-prefix + wait_for_selector + the failing set_fea_channel = 6; no trailing wait_for_idle.
    expect(calls).toHaveLength(6);
    expect(calls[5]).toEqual({ method: "set_fea_channel", args: { channel: "synthetic_channel" } });
  });

  it("(d) trailing wait_for_idle-after-feaChannelActions failure: failedLabel carries the trailing label", async () => {
    const { deps, calls, logs } = makeFakeDeps({
      // index 0 = the common-prefix wait_for_idle (succeeds); index 1 = the
      // trailing "after feaChannelActions" wait_for_idle (fails).
      wait_for_idle: [{ ok: true, value: null }, { ok: false, error: "stuck after channel switch" }],
    });
    const outcome = await runScenarioSteps(deps, SYNTHETIC_FEA_CHANNEL, REPO_ROOT);
    expect(outcome).toEqual({
      ok: false,
      failedLabel: "wait_for_idle after feaChannelActions",
      error: "stuck after channel switch",
    });
    expect(logs.some((l) => l.includes("FAIL wait_for_idle after feaChannelActions"))).toBe(true);
    expect(calls).toHaveLength(7);
  });
});

describe("runScenarioSteps — feaView block (task 2968)", () => {
  it("(a) deformed scenario appends toggle click, preset wait, preset click, then wait_for_idle, outcome ok", async () => {
    const { deps, calls } = makeFakeDeps();
    const outcome = await runScenarioSteps(deps, SYNTHETIC_FEA_VIEW_DEFORMED, REPO_ROOT);
    expect(calls).toHaveLength(8);
    expect(calls.slice(4)).toEqual([
      { method: "click_element", args: { testId: "fea-mode-show-deformed-toggle" } },
      { method: "wait_for_selector", args: { testId: "fea-mode-warp-preset-10" } },
      { method: "click_element", args: { testId: "fea-mode-warp-preset-10" } },
      { method: "wait_for_idle", args: { timeout_ms: 30_000 } },
    ]);
    expect(outcome).toEqual({ ok: true });
  });

  it("(b) contour scenario (deformed:false) appends no feaView actions — calls are the 4-call common prefix only", async () => {
    const { deps, calls } = makeFakeDeps();
    const outcome = await runScenarioSteps(deps, SYNTHETIC_FEA_VIEW_CONTOUR, REPO_ROOT);
    expect(calls).toHaveLength(4);
    expect(outcome).toEqual({ ok: true });
  });

  it("(c) first click_element failure (show-deformed toggle): subsequent steps and trailing wait_for_idle NOT recorded", async () => {
    const { deps, calls } = makeFakeDeps({
      click_element: { ok: false, error: "toggle missing" },
    });
    const outcome = await runScenarioSteps(deps, SYNTHETIC_FEA_VIEW_DEFORMED, REPO_ROOT);
    expect(outcome.ok).toBe(false);
    if (!outcome.ok) {
      expect(outcome.failedLabel).toBe("click_element(fea-mode-show-deformed-toggle)");
    }
    // 4 common-prefix calls (their own wait_for_idle included) + the failing
    // click_element call = 5; no feaView wait_for_selector, second
    // click_element, or trailing "wait_for_idle after feaViewActions".
    expect(calls).toHaveLength(5);
    expect(calls[4]).toEqual({ method: "click_element", args: { testId: "fea-mode-show-deformed-toggle" } });
    expect(calls.some((c) => c.method === "wait_for_selector")).toBe(false);
    expect(calls.filter((c) => c.method === "wait_for_idle")).toHaveLength(1);
  });

  it("(d) second click_element failure (warp preset): trailing wait_for_idle NOT recorded, failedLabel names the preset testId", async () => {
    const { deps, calls, logs } = makeFakeDeps({
      // index 0 = the show-deformed toggle click (succeeds); index 1 = the
      // warp-preset click (fails).
      click_element: [{ ok: true, value: null }, { ok: false, error: "preset click failed" }],
    });
    const outcome = await runScenarioSteps(deps, SYNTHETIC_FEA_VIEW_DEFORMED, REPO_ROOT);
    expect(outcome).toEqual({
      ok: false,
      failedLabel: "click_element(fea-mode-warp-preset-10)",
      error: "preset click failed",
    });
    expect(logs.some((l) => l.includes("FAIL click_element(fea-mode-warp-preset-10)"))).toBe(true);
    // 4 common-prefix + toggle click + preset wait_for_selector + the failing preset
    // click = 7; no trailing "wait_for_idle after feaViewActions".
    expect(calls).toHaveLength(7);
    expect(calls.filter((c) => c.method === "wait_for_idle")).toHaveLength(1);
  });

  it("(e) trailing wait_for_idle-after-feaViewActions failure: failedLabel carries the trailing label", async () => {
    const { deps, calls, logs } = makeFakeDeps({
      // index 0 = the common-prefix wait_for_idle (succeeds); index 1 = the
      // trailing "after feaViewActions" wait_for_idle (fails).
      wait_for_idle: [{ ok: true, value: null }, { ok: false, error: "stuck after view change" }],
    });
    const outcome = await runScenarioSteps(deps, SYNTHETIC_FEA_VIEW_DEFORMED, REPO_ROOT);
    expect(outcome).toEqual({
      ok: false,
      failedLabel: "wait_for_idle after feaViewActions",
      error: "stuck after view change",
    });
    expect(logs.some((l) => l.includes("FAIL wait_for_idle after feaViewActions"))).toBe(true);
    expect(calls).toHaveLength(8);
  });
});

describe("runScenarioSteps — feaChannel + feaView combined (repeated-method call-index coverage)", () => {
  it("second wait_for_selector failure (warp preset, after the feaChannel dropdown's own wait_for_selector): failedLabel names the preset testId, later feaView steps NOT recorded", async () => {
    const { deps, calls, logs } = makeFakeDeps({
      // index 0 = the feaChannel dropdown's wait_for_selector (succeeds);
      // index 1 = the feaView warp-preset wait_for_selector (fails).
      wait_for_selector: [{ ok: true, value: null }, { ok: false, error: "preset selector timeout" }],
    });
    const outcome = await runScenarioSteps(deps, SYNTHETIC_FEA_CHANNEL_AND_VIEW, REPO_ROOT);
    expect(outcome).toEqual({
      ok: false,
      failedLabel: "wait_for_selector(fea-mode-warp-preset-10)",
      error: "preset selector timeout",
    });
    expect(logs.some((l) => l.includes("FAIL wait_for_selector(fea-mode-warp-preset-10)"))).toBe(true);
    // 4 common-prefix + wait_for_selector(channel-select) + set_fea_channel + wait_for_idle
    // (feaChannel trailing) + click_element(toggle) + the failing preset wait_for_selector = 9;
    // no preset click_element or trailing "wait_for_idle after feaViewActions".
    expect(calls).toHaveLength(9);
    expect(
      calls.some((c) => c.method === "click_element" && c.args.testId === "fea-mode-warp-preset-10"),
    ).toBe(false);
  });
});
