// Visual regression driver for reify-gui.
//
// Spawns the GUI via scripts/run-gui-dev.sh, drives the debug MCP at
// 127.0.0.1:3939 over JSON-RPC, captures named screenshots, diffs them
// against PNG baselines under gui/test/screenshots/, and exits non-zero
// when a diff exceeds the configured tolerance.
//
//   npm run test:visual                    # run, fail on diff
//   UPDATE_BASELINES=1 npm run test:visual # rewrite all baselines
//
// Exit codes:
//   0 — all scenarios passed (or all baselines were created)
//   1 — at least one scenario diffed beyond tolerance
//   2 — harness error (gui failed to start, debug server unreachable, ...)
//
// A failing scenario writes <name>.actual.png and <name>.diff.png next to
// the baseline so the divergence can be inspected without re-running.

import { spawn, type ChildProcess } from "node:child_process";
import { existsSync } from "node:fs";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import pixelmatch from "pixelmatch";
import { PNG } from "pngjs";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

const REPO_ROOT = resolve(__dirname, "..", "..");
const SCREENSHOTS_DIR = resolve(__dirname, "screenshots");
const DEBUG_BASE = "http://127.0.0.1:3939";

const UPDATE_BASELINES = process.env.UPDATE_BASELINES === "1";

// Per-pixel YIQ tolerance used by pixelmatch (0..1, lower = stricter).
// 0.1 absorbs the small AA / driver differences that show up across machines
// without masking real geometry/shading regressions.
const PIXEL_THRESHOLD = 0.1;

// Cap on the fraction of pixels allowed to differ. The PRD asks for an
// SSIM-style tolerance of ≥0.99 across machines; ≤1% mismatched pixels at
// PIXEL_THRESHOLD=0.1 is the pixelmatch analog.
const MISMATCH_PCT_LIMIT = 0.01;

// How long we'll wait for the debug listener to come up after spawning
// run-gui-dev.sh. The script itself does an npm install + cargo build on a
// fresh checkout, so the budget is generous.
const SERVER_READY_TIMEOUT_MS = 10 * 60 * 1000;

interface Scenario {
    name: string;
    fixture: string; // path relative to REPO_ROOT
    camera: {
        position: [number, number, number];
        target: [number, number, number];
        up?: [number, number, number];
        zoom?: number;
    };
}

const SCENARIOS: Scenario[] = [
    {
        name: "m5_geometry_flange",
        fixture: "examples/m5_geometry.ri",
        camera: {
            position: [0.15, 0.1, 0.15],
            target: [0, 0, 0],
        },
    },
];

type RpcOk<T> = { ok: true; value: T };
type RpcErr = { ok: false; error: string };
type RpcResult<T> = RpcOk<T> | RpcErr;

let nextRpcId = 1;

async function rpc<T = unknown>(
    method: string,
    args: Record<string, unknown> = {},
): Promise<RpcResult<T>> {
    const body = JSON.stringify({
        jsonrpc: "2.0",
        id: nextRpcId++,
        method: "tools/call",
        params: { name: method, arguments: args },
    });
    let res: Response;
    try {
        res = await fetch(`${DEBUG_BASE}/mcp`, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body,
        });
    } catch (e) {
        return { ok: false, error: `network: ${(e as Error).message}` };
    }
    if (!res.ok) {
        return { ok: false, error: `http ${res.status}` };
    }
    const envelope = (await res.json()) as {
        error?: { code: number; message: string };
        result?: {
            content?: Array<{ type: string; text?: string; data?: string }>;
            isError?: boolean;
        };
    };
    if (envelope.error) {
        return { ok: false, error: envelope.error.message };
    }
    const result = envelope.result;
    const content = result?.content?.[0];
    if (result?.isError) {
        return { ok: false, error: content?.text ?? "(unknown error)" };
    }
    if (content?.type === "image" && content.data) {
        return { ok: true, value: { data: content.data } as unknown as T };
    }
    if (content?.type === "text" && typeof content.text === "string") {
        try {
            return { ok: true, value: JSON.parse(content.text) as T };
        } catch {
            return { ok: true, value: content.text as unknown as T };
        }
    }
    return { ok: true, value: (result ?? {}) as T };
}

async function waitForDebugServer(timeoutMs: number): Promise<boolean> {
    const start = Date.now();
    while (Date.now() - start < timeoutMs) {
        const r = await rpc("health");
        if (r.ok) return true;
        await sleep(1000);
    }
    return false;
}

interface CapturedImage {
    width: number;
    height: number;
    pngBuffer: Buffer;
    rgba: Buffer;
}

function decodePng(buf: Buffer): CapturedImage {
    const png = PNG.sync.read(buf);
    return { width: png.width, height: png.height, pngBuffer: buf, rgba: png.data };
}

async function takeScreenshot(): Promise<CapturedImage> {
    const r = await rpc<{ data: string }>("screenshot");
    if (!r.ok) throw new Error(`screenshot rpc failed: ${r.error}`);
    return decodePng(Buffer.from(r.value.data, "base64"));
}

async function readBaseline(path: string): Promise<CapturedImage | null> {
    if (!existsSync(path)) return null;
    return decodePng(await readFile(path));
}

type ScenarioOutcome =
    | { status: "passed"; detail: string }
    | { status: "baseline-created"; detail: string }
    | { status: "failed"; detail: string };

async function runScenario(scenario: Scenario): Promise<ScenarioOutcome> {
    const fixturePath = resolve(REPO_ROOT, scenario.fixture);

    const open = await rpc("open_file", { path: fixturePath });
    if (!open.ok) return { status: "failed", detail: `open_file: ${open.error}` };

    const tm = await rpc("set_test_mode", { enabled: true });
    if (!tm.ok) return { status: "failed", detail: `set_test_mode: ${tm.error}` };

    const camArgs: Record<string, unknown> = {
        position: scenario.camera.position,
        target: scenario.camera.target,
    };
    if (scenario.camera.up) camArgs.up = scenario.camera.up;
    if (scenario.camera.zoom != null) camArgs.zoom = scenario.camera.zoom;
    const cam = await rpc("set_camera", camArgs);
    if (!cam.ok) return { status: "failed", detail: `set_camera: ${cam.error}` };

    const idle = await rpc("wait_for_idle", { timeout_ms: 30_000 });
    if (!idle.ok) return { status: "failed", detail: `wait_for_idle: ${idle.error}` };

    const captured = await takeScreenshot();

    await mkdir(SCREENSHOTS_DIR, { recursive: true });
    const baselinePath = join(SCREENSHOTS_DIR, `${scenario.name}.png`);
    const baseline = await readBaseline(baselinePath);

    if (!baseline || UPDATE_BASELINES) {
        await writeFile(baselinePath, captured.pngBuffer);
        return {
            status: "baseline-created",
            detail: baseline
                ? `baseline updated (UPDATE_BASELINES=1) at ${baselinePath}`
                : `baseline created at ${baselinePath}`,
        };
    }

    if (baseline.width !== captured.width || baseline.height !== captured.height) {
        const actualPath = join(SCREENSHOTS_DIR, `${scenario.name}.actual.png`);
        await writeFile(actualPath, captured.pngBuffer);
        return {
            status: "failed",
            detail:
                `dimension mismatch: baseline ${baseline.width}x${baseline.height} ` +
                `vs captured ${captured.width}x${captured.height}; wrote ${actualPath}`,
        };
    }

    const diff = new PNG({ width: captured.width, height: captured.height });
    const mismatchedPixels = pixelmatch(
        baseline.rgba,
        captured.rgba,
        diff.data,
        captured.width,
        captured.height,
        { threshold: PIXEL_THRESHOLD },
    );
    const totalPixels = captured.width * captured.height;
    const mismatchPct = mismatchedPixels / totalPixels;

    if (mismatchPct > MISMATCH_PCT_LIMIT) {
        const actualPath = join(SCREENSHOTS_DIR, `${scenario.name}.actual.png`);
        const diffPath = join(SCREENSHOTS_DIR, `${scenario.name}.diff.png`);
        await writeFile(actualPath, captured.pngBuffer);
        await writeFile(diffPath, PNG.sync.write(diff));
        return {
            status: "failed",
            detail:
                `${mismatchedPixels} px (${(mismatchPct * 100).toFixed(2)}%) differ ` +
                `(limit ${(MISMATCH_PCT_LIMIT * 100).toFixed(2)}%); wrote ${actualPath} and ${diffPath}`,
        };
    }

    return {
        status: "passed",
        detail: `${mismatchedPixels} px (${(mismatchPct * 100).toFixed(3)}%) within tolerance`,
    };
}

function spawnGui(launchFixture: string): ChildProcess {
    return spawn("scripts/run-gui-dev.sh", [launchFixture], {
        cwd: REPO_ROOT,
        stdio: ["ignore", "inherit", "inherit"],
        env: process.env,
        detached: false,
    });
}

async function reapGui(gui: ChildProcess): Promise<void> {
    if (!gui.pid || gui.killed) return;
    gui.kill("SIGTERM");
    await new Promise<void>((done) => {
        const timer = setTimeout(() => {
            gui.kill("SIGKILL");
            done();
        }, 5000);
        gui.once("exit", () => {
            clearTimeout(timer);
            done();
        });
    });
}

async function main(): Promise<number> {
    console.log("==> visual-regression harness");
    console.log(`    REPO_ROOT=${REPO_ROOT}`);
    console.log(`    SCREENSHOTS_DIR=${SCREENSHOTS_DIR}`);
    console.log(`    UPDATE_BASELINES=${UPDATE_BASELINES ? "1" : "0"}`);

    if (SCENARIOS.length === 0) {
        console.error("==> no scenarios defined");
        return 2;
    }

    const launchFixture = resolve(REPO_ROOT, SCENARIOS[0].fixture);
    if (!existsSync(launchFixture)) {
        console.error(`==> launch fixture missing: ${launchFixture}`);
        return 2;
    }

    console.log(`==> launching reify-gui with ${launchFixture}`);
    const gui = spawnGui(launchFixture);

    const failures: string[] = [];
    let exitCode = 0;

    try {
        if (!(await waitForDebugServer(SERVER_READY_TIMEOUT_MS))) {
            throw new Error(
                `debug server at ${DEBUG_BASE} did not become ready within ` +
                    `${Math.round(SERVER_READY_TIMEOUT_MS / 1000)}s`,
            );
        }
        console.log("==> debug server ready");

        for (const scenario of SCENARIOS) {
            console.log(`==> scenario: ${scenario.name}`);
            const r = await runScenario(scenario);
            const tag =
                r.status === "passed"
                    ? "PASS"
                    : r.status === "baseline-created"
                      ? "BASE"
                      : "FAIL";
            console.log(`    [${tag}] ${scenario.name} — ${r.detail}`);
            if (r.status === "failed") failures.push(scenario.name);
        }

        if (failures.length > 0) {
            console.error(`==> ${failures.length} scenario(s) failed: ${failures.join(", ")}`);
            exitCode = 1;
        } else {
            console.log("==> all scenarios passed");
        }
    } catch (e) {
        console.error(`==> harness error: ${(e as Error).message}`);
        exitCode = 2;
    } finally {
        await reapGui(gui);
    }

    return exitCode;
}

main().then(
    (code) => process.exit(code),
    (e) => {
        console.error(e);
        process.exit(2);
    },
);
