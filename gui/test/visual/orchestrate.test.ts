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
 * Build a recording fake rpc + log pair for runScenarioSteps tests.
 *
 * `scripted` maps a method name to the RpcResult it should return; any
 * unscripted method call defaults to {ok:true, value:null}. Every call
 * (scripted or not) is pushed onto `calls`, in invocation order, before the
 * scripted result is returned — so a scripted failure still shows up in
 * `calls` (the call itself was made; it just failed).
 */
function makeFakeDeps(scripted: Partial<Record<string, RpcResult<unknown>>> = {}) {
  const calls: Call[] = [];
  const logs: string[] = [];
  const rpc: RpcFn = async <T>(method: string, args: Record<string, unknown>): Promise<RpcResult<T>> => {
    calls.push({ method, args });
    return (scripted[method] ?? { ok: true, value: null }) as RpcResult<T>;
  };
  const log = (m: string) => logs.push(m);
  const deps: ScenarioRunDeps = { rpc, log };
  return { deps, calls, logs };
}

describe("runScenarioSteps — common prefix (open_file, set_test_mode, set_camera, wait_for_idle)", () => {
  const plain = SCENARIOS.find((s) => s.name === "m5_geometry_flange")!;

  it("(a) plain scenario emits exactly the 4-call common prefix and returns ok:true", async () => {
    const { deps, calls } = makeFakeDeps();
    const outcome: ScenarioStepsOutcome = await runScenarioSteps(deps, plain, REPO_ROOT);
    expect(calls).toEqual([
      { method: "open_file", args: { path: "/repo/examples/m5_geometry_flange.ri" } },
      { method: "set_test_mode", args: { enabled: true } },
      { method: "set_camera", args: { position: [0.15, 0.1, 0.15], target: [0, 0, 0] } },
      { method: "wait_for_idle", args: { timeout_ms: 30_000 } },
    ]);
    expect(outcome).toEqual({ ok: true });
  });

  it("(b) set_camera args carry no up/zoom keys when the scenario camera omits them", async () => {
    const { deps, calls } = makeFakeDeps();
    await runScenarioSteps(deps, plain, REPO_ROOT);
    const cameraCall = calls.find((c) => c.method === "set_camera")!;
    expect(Object.keys(cameraCall.args)).toEqual(["position", "target"]);
  });

  it("(c) set_camera args include up and zoom when the scenario camera sets them", async () => {
    const withUpZoom: Scenario = {
      name: "synthetic_camera_up_zoom",
      fixture: "examples/synthetic.ri",
      camera: {
        position: [1, 2, 3],
        target: [0, 0, 0],
        up: [0, 1, 0],
        zoom: 2.5,
      },
    };
    const { deps, calls } = makeFakeDeps();
    const outcome = await runScenarioSteps(deps, withUpZoom, REPO_ROOT);
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
    const outcome = await runScenarioSteps(deps, plain, REPO_ROOT);
    expect(outcome).toEqual({ ok: false, failedLabel: "open_file", error: "boom" });
    expect(logs.some((l) => l.includes("FAIL open_file"))).toBe(true);
    expect(calls).toEqual([{ method: "open_file", args: { path: "/repo/examples/m5_geometry_flange.ri" } }]);
  });

  it("(e) mid-prefix failure (set_camera): wait_for_idle is never called and failedLabel is set_camera", async () => {
    const { deps, calls } = makeFakeDeps({
      set_camera: { ok: false, error: "camera exploded" },
    });
    const outcome = await runScenarioSteps(deps, plain, REPO_ROOT);
    expect(outcome.ok).toBe(false);
    if (!outcome.ok) {
      expect(outcome.failedLabel).toBe("set_camera");
    }
    expect(calls.some((c) => c.method === "wait_for_idle")).toBe(false);
  });
});
