import { spawn, spawnSync, type ChildProcessWithoutNullStreams } from 'node:child_process';
import * as fs from 'node:fs';
import { isBuiltin } from 'node:module';
import * as os from 'node:os';
import * as path from 'node:path';
import type { Readable } from 'node:stream';
import { fileURLToPath } from 'node:url';
import { afterAll, afterEach, beforeAll, describe, expect, it } from 'vitest';
import { createLineReader } from '../ipc.js';
import type { InboundMessage, OutboundMessage } from '../types.js';

// Guards leodearden/reify#4. The sidecar ships as a Tauri externalBin (build-sidecar.sh copies it
// to src-tauri/sidecar/), so no node_modules is reachable from where it runs and tsup.sidecar.ts
// must bundle every dependency that src/ imports. This builds the real config and checks the
// result statically, then runs it installed the way Tauri deploys it.

const SIDECAR_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const SIDECAR_EXECUTABLE = 'reify-sidecar';
const BUILD_TIMEOUT_MS = 60_000;
const RUN_TIMEOUT_MS = 30_000;
const TURN_ID = 't1';

const FAKE_CLAUDE_STREAM: readonly object[] = [
  { type: 'assistant', message: { id: 'm1', content: [{ type: 'text', text: 'PONG' }] } },
  { type: 'result', session_id: 's1' },
];

interface BundleMetafile {
  outputs: Record<string, { imports: Array<{ path: string; kind: string; external?: boolean }> }>;
}

interface BuiltBundle {
  bundlePath: string;
  metafile: BundleMetafile;
}

interface ProcessExit {
  code: number | null;
  signal: NodeJS.Signals | null;
  stderr: string;
}

const scratchDirs: string[] = [];
const launched: SidecarProcess[] = [];

function makeScratchDir(prefix: string): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), prefix));
  scratchDirs.push(dir);
  return dir;
}

function buildBundle(outDir: string): BuiltBundle {
  // tsup externalizes the dependencies declared in the package.json of its cwd, so the cwd must
  // be the sidecar package or a broken config could pass.
  const tsup = path.join(SIDECAR_ROOT, 'node_modules', '.bin', 'tsup');
  const build = spawnSync(tsup, ['--config', 'tsup.sidecar.ts', '--out-dir', outDir, '--metafile', '--silent'], {
    cwd: SIDECAR_ROOT,
    encoding: 'utf8',
  });
  if (build.status !== 0) {
    throw new Error(
      `tsup build failed (status ${build.status}, signal ${build.signal}): ${build.error ?? ''}\n${build.stdout}${build.stderr}`,
    );
  }
  return {
    bundlePath: path.join(outDir, 'cli.js'),
    metafile: JSON.parse(fs.readFileSync(path.join(outDir, 'metafile-esm.json'), 'utf8')) as BundleMetafile,
  };
}

function externalImports(metafile: BundleMetafile): string[] {
  return Object.values(metafile.outputs).flatMap((output) =>
    output.imports.filter((entry) => entry.external === true).map((entry) => entry.path),
  );
}

/** The last step of build-sidecar.sh: shebang + bundle, chmod +x, which Tauri copies beside the GUI binary. */
function installAsExternalBin(bundlePath: string, installDir: string): void {
  const executable = path.join(installDir, SIDECAR_EXECUTABLE);
  fs.writeFileSync(executable, Buffer.concat([Buffer.from('#!/usr/bin/env node\n'), fs.readFileSync(bundlePath)]));
  fs.chmodSync(executable, 0o755);
}

function ancestorDirs(dir: string): string[] {
  const parent = path.dirname(dir);
  return parent === dir ? [dir] : [dir, ...ancestorDirs(parent)];
}

function nodeModulesAncestors(dir: string): string[] {
  return ancestorDirs(path.resolve(dir)).filter((ancestor) => fs.existsSync(path.join(ancestor, 'node_modules')));
}

function installIntoBareDirectory(bundlePath: string): string {
  const installDir = makeScratchDir('reify-sidecar-install-');
  expect(
    nodeModulesAncestors(installDir),
    'a node_modules at or above the install dir would let a missing dependency resolve',
  ).toEqual([]);
  installAsExternalBin(bundlePath, installDir);
  return installDir;
}

/** A `claude` that drains the prompt on stdin and replies with the given stream-json events. */
function installFakeClaude(streamEvents: readonly object[]): string {
  const binDir = makeScratchDir('reify-sidecar-fakebin-');
  const script = ['#!/bin/sh', 'cat > /dev/null', "cat <<'EVENTS'", ...streamEvents.map((event) => JSON.stringify(event)), 'EVENTS', ''];
  const claude = path.join(binDir, 'claude');
  fs.writeFileSync(claude, script.join('\n'));
  fs.chmodSync(claude, 0o755);
  return binDir;
}

/**
 * Execs the installed sidecar directly, as the Rust host does, under a minimal environment so
 * nothing inherited (NODE_PATH, NODE_OPTIONS, REIFY_LANDLOCK_EXEC, ...) can change module
 * resolution or the code path.
 */
function launchSidecar(installDir: string, pathDirs: string[] = []): SidecarProcess {
  const sidecar = new SidecarProcess(path.join(installDir, SIDECAR_EXECUTABLE), installDir, {
    PATH: [...pathDirs, path.dirname(process.execPath), '/usr/bin', '/bin'].join(path.delimiter),
    HOME: os.homedir(),
    REIFY_WORKSPACE: installDir,
    // session.ts makes reify-mcp-* dirs under TMPDIR. A private one keeps them out of the shared /tmp
    // that session.test.ts's leak guard watches, and removes them even when the child is SIGKILLed.
    TMPDIR: makeScratchDir('reify-sidecar-tmp-'),
  });
  launched.push(sidecar);
  return sidecar;
}

async function* parseOutbound(stdout: Readable): AsyncGenerator<OutboundMessage> {
  for await (const line of createLineReader(stdout)) {
    yield JSON.parse(line) as OutboundMessage;
  }
}

/** Drives a sidecar child process over its newline-delimited JSON stdio protocol. */
class SidecarProcess {
  private readonly child: ChildProcessWithoutNullStreams;
  private readonly outbound: AsyncGenerator<OutboundMessage>;
  private readonly received: OutboundMessage[] = [];
  private readonly closed: Promise<ProcessExit>;
  private stderr = '';

  constructor(executable: string, cwd: string, env: NodeJS.ProcessEnv) {
    this.child = spawn(executable, [], { cwd, env, stdio: 'pipe' });
    this.outbound = parseOutbound(this.child.stdout);
    this.child.stderr.setEncoding('utf8');
    this.child.stderr.on('data', (chunk: string) => {
      this.stderr += chunk;
    });
    // A write racing the child's death raises EPIPE; receiveUntil reports the exit instead.
    this.child.stdin.on('error', () => {});
    this.closed = new Promise((resolve) => {
      this.child.on('error', (error) => {
        this.stderr += `${error.message}\n`;
      });
      this.child.on('close', (code, signal) => resolve({ code, signal, stderr: this.stderr }));
    });
  }

  send(message: InboundMessage): void {
    this.child.stdin.write(JSON.stringify(message) + '\n');
  }

  /** Reads outbound messages until `isComplete` holds over everything received so far; fails if the child exits first. */
  async receiveUntil(
    isComplete: (received: readonly OutboundMessage[]) => boolean,
  ): Promise<readonly OutboundMessage[]> {
    while (!isComplete(this.received)) {
      const next = await this.outbound.next();
      if (next.done) throw await this.exitedEarly();
      this.received.push(next.value);
    }
    return [...this.received];
  }

  /** Closes stdin, the shutdown signal the host relies on, and resolves once the process has exited. */
  endInput(): Promise<ProcessExit> {
    this.child.stdin.end();
    return this.closed;
  }

  kill(): void {
    this.child.kill('SIGKILL');
  }

  private async exitedEarly(): Promise<Error> {
    const { code, signal, stderr } = await this.closed;
    return new Error(
      `sidecar exited (code ${code}, signal ${signal}) before sending what the test waited for\n` +
        `messages received: ${JSON.stringify(this.received)}\nstderr:\n${stderr}`,
    );
  }
}

function turnMessages(messages: readonly OutboundMessage[], turnId: string): OutboundMessage[] {
  return messages.filter((message) => 'id' in message && message.id === turnId);
}

function endsTurn(message: OutboundMessage): boolean {
  return message.type === 'done' || message.type === 'error';
}

function includesReady(messages: readonly OutboundMessage[]): boolean {
  return messages.some((message) => message.type === 'ready');
}

describe('the sidecar bundle built from tsup.sidecar.ts', () => {
  let bundle: BuiltBundle;

  beforeAll(() => {
    bundle = buildBundle(makeScratchDir('reify-sidecar-bundle-'));
  }, BUILD_TIMEOUT_MS);

  afterEach(() => {
    for (const sidecar of launched.splice(0)) sidecar.kill();
  });

  afterAll(() => {
    for (const dir of scratchDirs.splice(0)) fs.rmSync(dir, { recursive: true, force: true });
  });

  it('the built bundle imports nothing but Node built-ins', () => {
    const externals = externalImports(bundle.metafile);

    expect(externals, 'no external imports at all means the metafile shape changed').not.toEqual([]);
    expect(externals.filter((specifier) => !isBuiltin(specifier))).toEqual([]);
  });

  it('the installed sidecar completes the ready handshake from a directory with no node_modules above it', async () => {
    const sidecar = launchSidecar(installIntoBareDirectory(bundle.bundlePath));

    const [first] = await sidecar.receiveUntil((received) => received.length >= 1);
    expect(first).toEqual({ type: 'ready' });

    const exit = await sidecar.endInput();
    expect({ code: exit.code, signal: exit.signal }, `sidecar stderr:\n${exit.stderr}`).toEqual({
      code: 0,
      signal: null,
    });
  }, RUN_TIMEOUT_MS);

  it('the installed sidecar still spawns the claude CLI from PATH', async () => {
    const fakeBin = installFakeClaude(FAKE_CLAUDE_STREAM);
    const sidecar = launchSidecar(installIntoBareDirectory(bundle.bundlePath), [fakeBin]);
    await sidecar.receiveUntil(includesReady);

    sidecar.send({ type: 'send_message', id: TURN_ID, text: 'ping' });
    const received = await sidecar.receiveUntil((messages) => turnMessages(messages, TURN_ID).some(endsTurn));

    expect(turnMessages(received, TURN_ID)).toEqual([
      { type: 'text_delta', id: TURN_ID, content: 'PONG' },
      { type: 'done', id: TURN_ID },
    ]);
  }, RUN_TIMEOUT_MS);
});
