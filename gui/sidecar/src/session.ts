import { spawn, type ChildProcess } from 'node:child_process';
import * as fs from 'node:fs';
import * as path from 'node:path';
import * as os from 'node:os';
import { createLineReader } from './ipc.js';
import type { InboundMessage, NoticeMessage, OutboundMessage, SendMessage } from './types.js';
import type { PermissionServer } from './permission-server.js';
import { wrapClaudeArgs } from './sandbox.js';
import { sidecarMcpServers } from './tool-surface.js';
import { buildClaudeArgs } from './claudeArgs.js';
import {
  forgetToolUses,
  initialStreamState,
  mapStreamEvent,
  type StreamState,
} from './streamEvents.js';
import { errorMessage } from './utils.js';

export interface SessionConfig {
  model: string;
  workingDirectory: string;
  systemPrompt: string;
  /** Timeout in milliseconds for each SDK invocation. Default: 300_000 (5 minutes). */
  timeoutMs?: number;
  /**
   * Optional permission-prompt MCP server to wire Claude CLI's --permission-prompt-tool.
   * When provided, buildClaudeArgs adds --permission-prompt-tool and the MCP config
   * gains a reify-permission entry.
   */
  permissionMcp?: {
    /** The MCP endpoint URL (e.g. http://127.0.0.1:<port>/mcp) */
    url: string;
    /** The PermissionServer instance for registering the onRequest callback */
    server: PermissionServer;
  };
  /**
   * The writable workspace directory for the landlock sandbox.
   * Falls back to `workingDirectory` when not set.
   * Set from the REIFY_WORKSPACE env var by index.ts at startup.
   */
  workspace?: string;
  /**
   * Path to the vendored landlock_exec.py helper script.
   * When set and the kernel supports landlock, the claude child is wrapped with
   * `python3 <landlockExec> --writable <workspace> --writable ~/.claude --writable /tmp -- claude <args>`.
   * When absent, no sandbox is applied (silent direct spawn).
   * Set from the REIFY_LANDLOCK_EXEC env var by index.ts at startup.
   */
  landlockExec?: string;
  /**
   * Whether the kernel supports Landlock FS sandboxing.
   * Resolved once at sidecar startup by `probeLandlockAsync()` in index.ts and passed
   * here as an eagerly-settled boolean so invokeSdk can read it synchronously.
   * Defaults to `false` when omitted (e.g. in unit tests that do not set it).
   */
  landlockAvailable?: boolean;
  /**
   * The reify-debug MCP endpoint, set ONLY when the host reports its own reify-debug
   * server live (REIFY_DEBUG_MCP_URL). Absent ⇒ no reify-debug MCP entry and no
   * mcp__reify-debug__ grant.
   */
  reifyDebugMcpUrl?: string;
}

/**
 * Diagnostic log prefix used by the proc.on('error') handler for non-ABORT spawn errors.
 * Exported so tests can reference it without hardcoding the string literal.
 */
export const SPAWN_ERROR_LOG_PREFIX = '[sidecar] spawned claude error:';

/**
 * Manages a Claude Code SDK session, dispatching inbound messages
 * and emitting outbound messages via the onOutput callback.
 *
 * Spawns the claude CLI in stream-json print mode (argv: claudeArgs.ts::buildClaudeArgs),
 * maps its stdout through streamEvents.ts::mapStreamEvent, and resumes with the CLI
 * session id for conversation continuity.
 */
export class SidecarSession {
  private config: SessionConfig;
  private sessionId: string | null = null;
  private abortController: AbortController | null = null;
  private destroyed = false;

  /** The in-flight invocation's stream state (delta offsets, tool_use correlation), if any. */
  private stream: StreamState | null = null;
  /** The stdin of the currently in-flight claude CLI process, if any. */
  private currentStdin: NodeJS.WritableStream | null = null;

  /**
   * Maps request_id → tool_name for pending permission requests.
   * Used to look up the tool_name when a permission_decision with remember:true arrives.
   */
  private pendingPermissionRequests: Map<string, string> = new Map();

  /**
   * Cached MCP config file path and its parent temp directory.
   * Written once on the first invokeSdk call (lazily) and cleaned up in destroy().
   * Caching avoids repeated mkdtemp+writeFile+unlink per turn for a fixed-lifetime URL.
   */
  private mcpConfigTmpDir: string | null = null;
  private mcpConfigTmpFile: string | null = null;

  /**
   * Whether the one-shot sandbox_unavailable notice has already been emitted for this session.
   * Guards against emitting the warning on every subsequent turn after the first failed probe.
   */
  private sandboxNoticeEmitted = false;

  /** The send_message id for the currently in-flight invocation, if any. */
  private currentInvocationId: string | null = null;

  /** Called when the session produces an outbound message. */
  onOutput: (msg: OutboundMessage) => void = () => {};

  /** Returns true while a handleSendMessage invocation is in-flight. Exposed for tests. */
  isInvocationActive(): boolean {
    return this.abortController !== null;
  }

  constructor(config: SessionConfig) {
    this.config = config;
    // Register the permission-request handler immediately if a permission server is configured.
    // The handler reads currentInvocationId dynamically so it picks up the correct id for
    // whichever send_message invocation is in flight when the callback fires.
    // The matching deregistration happens in destroy() via onRequest(null).
    if (config.permissionMcp) {
      config.permissionMcp.server.onRequest((req) => {
        // Short-circuit any callbacks that race with destroy(): the session is
        // torn down, there is nothing to do and no safe onOutput to call.
        if (this.destroyed) return;
        // Guard: if no invocation is in-flight (e.g. a very late CLI request arriving
        // after done/abort, or before the first send_message), there is no host UI to
        // show the prompt and no message id to attach the permission_request to.
        // Emitting with id:'' silently degrades to an unattached prompt — instead we
        // deny immediately and emit a structured diagnostic notice so the host can log
        // the race for diagnosability. Do NOT register in pendingPermissionRequests
        // since there is no decision lifecycle to track.
        if (this.currentInvocationId === null) {
          config.permissionMcp!.server.decide(req.request_id, { behavior: 'deny' });
          this.emitNotice({
            type: 'notice',
            id: '',
            code: 'permission_request_orphaned',
            message: `Permission request for '${req.tool_name}' arrived outside an in-flight invocation and was automatically denied.`,
          });
          return;
        }
        const id = this.currentInvocationId;
        this.pendingPermissionRequests.set(req.request_id, req.tool_name);
        this.onOutput({
          type: 'permission_request',
          id,
          request_id: req.request_id,
          tool_name: req.tool_name,
          tool_input: req.tool_input,
        });
      });
    }
  }

  /**
   * Initialize the session and signal readiness.
   */
  async init(): Promise<void> {
    this.onOutput({ type: 'ready' });
  }

  /**
   * Dispose the session: abort any in-flight request and prevent further
   * message handling. Safe to call multiple times (idempotent).
   */
  destroy(): void {
    if (this.destroyed) return;
    this.destroyed = true;
    this.abortController?.abort();
    // Deregister the permission-request handler before cancelling, so a shared
    // PermissionServer (currently 1:1 in production index.ts, but defensible
    // against future refactors) does not retain a closure over this destroyed
    // session. The matching registration happens in the constructor.
    this.config.permissionMcp?.server.onRequest(null);
    // Cancel any pending permission requests so suspended HTTP handlers are unblocked.
    this.config.permissionMcp?.server.cancelAll();
    this.sessionId = null;
    this.pendingPermissionRequests.clear();
    this.currentInvocationId = null;
    // Clean up the cached MCP config file and its temp directory.
    if (this.mcpConfigTmpFile) {
      try { fs.unlinkSync(this.mcpConfigTmpFile); } catch {}
      this.mcpConfigTmpFile = null;
    }
    if (this.mcpConfigTmpDir) {
      try { fs.rmdirSync(this.mcpConfigTmpDir); } catch {}
      this.mcpConfigTmpDir = null;
    }
  }

  /**
   * Dispatch an inbound message to the appropriate handler.
   * Returns immediately (no-op) if the session has been destroyed.
   */
  async handleMessage(msg: InboundMessage): Promise<void> {
    if (this.destroyed) return;
    switch (msg.type) {
      case 'send_message':
        await this.handleSendMessage(msg.id, msg.text, msg.context);
        break;
      case 'abort':
        this.handleAbort();
        break;
      case 'clear_session':
        this.handleClearSession();
        break;
      case 'permission_decision':
        this.handlePermissionDecision(msg.request_id, msg.behavior, msg.message, msg.updated_input, msg.remember);
        break;
    }
  }

  /**
   * Process a user message through the Claude Code CLI in streaming mode.
   */
  private async handleSendMessage(
    id: string,
    text: string,
    context?: SendMessage['context']
  ): Promise<void> {
    // Build the prompt with optional context
    let prompt = text;
    if (context) {
      const contextParts: string[] = [];
      if (context.current_file) {
        contextParts.push(`Current file: ${context.current_file}`);
      }
      if (context.selected_entity) {
        contextParts.push(`Selected entity: ${context.selected_entity}`);
      }
      if (context.diagnostics?.length) {
        contextParts.push(`Diagnostics:\n${context.diagnostics.join('\n')}`);
      }
      if (context.constraints?.length) {
        contextParts.push(`Constraints:\n${context.constraints.join('\n')}`);
      }
      if (context.attached_contexts?.length) {
        contextParts.push(`Attached contexts:\n${context.attached_contexts.join('\n\n')}`);
      }
      if (contextParts.length > 0) {
        prompt = `${text}\n\n[Context]\n${contextParts.join('\n\n')}`;
      }
    }

    // Create abort controller for this request
    this.abortController = new AbortController();
    this.currentInvocationId = id;

    try {
      await this.invokeSdk(id, prompt);
      this.emitAbortOrDone(id);
    } catch (error: unknown) {
      if (this.abortController?.signal.aborted) {
        this.emitAbortOrDone(id);
      } else {
        const message = error instanceof Error ? error.message : String(error);
        this.onOutput({ type: 'error', id, message });
      }
    } finally {
      this.abortController = null;
      this.currentInvocationId = null;
      // Cancel any permission requests that were still pending when the invocation
      // ended (abort, error, or normal exit). Without this, a killed subprocess
      // leaves the approve_tool HTTP handlers suspended forever since the Claude CLI
      // closed its side of the connection but pendingPromises still holds the resolvers.
      this.config.permissionMcp?.server.cancelAll();
      this.pendingPermissionRequests.clear();
    }
  }

  /**
   * Invoke Claude Code CLI in streaming JSON mode and emit events.
   */
  private async invokeSdk(id: string, prompt: string): Promise<void> {
    const args = buildClaudeArgs(this.config, this.sessionId, this.mcpConfigPath());
    const { cmd, args: wrappedArgs } = this.sandboxedCommand(id, args);

    const proc = spawn(cmd, wrappedArgs, {
      cwd: this.config.workingDirectory,
      signal: this.abortController?.signal,
      stdio: ['pipe', 'pipe', 'pipe'],
    });

    // Required: 'error' listener on the ChildProcess itself (distinct from proc.stdin?.on('error')).
    //
    // When the spawn `signal` aborts (timeout setTimeout → abortController.abort('timeout'),
    // or user handleAbort), Node's abortChildProcess calls child.emit('error', err) with
    // err.code === 'ABORT_ERR' BEFORE the 'close' event (node:child_process abortChildProcess).
    // Per Node's EventEmitter contract, an unlistened-to 'error' event is rethrown via
    // `process.nextTick(() => { throw err })`, killing the entire sidecar process.
    //
    // ABORT_ERR is benign — it is the deliberate signal we caused. The close-path's exitCode
    // check (see proc.on('close', ...) below) plus emitAbortOrDone already produce the correct
    // outbound (timeout error or done). Swallow ABORT_ERR silently to avoid double-reporting.
    //
    // Other errors (ENOENT, EACCES — spawn-time failures before a 'close' is guaranteed) are
    // logged via console.error for diagnosability. The close-path exit code remains the
    // authoritative failure signal for outbound messages; do NOT emit an extra outbound here.
    // Mirrors the orphan-stream-error convention used by proc.stdin?.on('error', ...) below.
    proc.on('error', (err: Error & { code?: string }) => {
      if (err.code !== 'ABORT_ERR') {
        console.error(SPAWN_ERROR_LOG_PREFIX, err);
      }
    });

    // Attach an 'error' listener so an unhandled stream error cannot crash the sidecar process.
    // Error reporting for *correlated* writes flows through per-write callbacks (see writes below),
    // which capture the correct id for each write rather than the outer send_message id.
    //
    // Orphan stream errors (fired to the 'error' listener without a pending write callback, e.g.
    // when the underlying fd is closed externally) are now surfaced as a console.warn diagnostic
    // so they are diagnosable in production rather than dropped silently. The close path —
    // observable via proc.on('close') and the exitCode check below — remains the authoritative
    // failure signal. A host relying on EPIPE detection should check the 'done'/'error' outbound
    // from handleSendMessage instead.
    let warnedOrphanStdinError = false;
    proc.stdin?.on('error', (err: Error) => {
      // One-shot guard prevents log spam if the kernel/Node fires repeated errors on the
      // same stream after an external fd close.
      if (!warnedOrphanStdinError) {
        warnedOrphanStdinError = true;
        console.warn(`[sidecar] stdin error (orphan, no pending write): ${err.message}`);
      }
    });

    // Write the initial user prompt as a stream-json message line.
    // Stdin is kept open until a 'result' event arrives so tool_result blocks can follow.
    // Per-write callback captures the send_message id for correct error correlation.
    proc.stdin?.write(
      JSON.stringify({
        type: 'user',
        message: { role: 'user', content: [{ type: 'text', text: prompt }] },
      }) + '\n',
      (err) => {
        if (err) this.onOutput({ type: 'error', id, message: `stdin write error: ${err.message}` });
      }
    );

    // Register the in-flight stdin and a fresh stream state for this invocation.
    this.currentStdin = proc.stdin ?? null;
    this.stream = initialStreamState(id);

    // Start timeout timer
    const timeoutMs = this.config.timeoutMs ?? 300_000;
    const timeoutId = setTimeout(() => {
      this.abortController?.abort('timeout');
    }, timeoutMs);

    // Collect stderr for error reporting
    let stderr = '';
    proc.stderr?.on('data', (data: Buffer) => {
      stderr += data.toString();
    });

    // Capture exit code as soon as process closes (before stream parsing finishes)
    const exitPromise = new Promise<number | null>((resolve) => {
      proc.on('close', (code: number | null) => resolve(code));
    });

    try {
      await this.consumeStream(proc);
    } finally {
      clearTimeout(timeoutId);
      this.currentStdin = null;
      this.stream = null;
    }

    // Wait for process exit and check exit code
    const exitCode = await exitPromise;
    if (exitCode !== 0 && exitCode !== null) {
      throw new Error(stderr || `Claude CLI exited with code ${exitCode}`);
    }
  }

  /**
   * Read the claude child's stream-json stdout to its end, applying each event. A line
   * that is not JSON is skipped with a warning. If applying an event throws, the child is
   * killed (it would otherwise keep running tools for a turn that has already failed)
   * and the error propagates, so the turn ends with an error outbound.
   */
  private async consumeStream(proc: ChildProcess): Promise<void> {
    for await (const line of createLineReader(proc.stdout!)) {
      let event: unknown;
      try {
        event = JSON.parse(line);
      } catch (parseError) {
        console.warn(`[sidecar] skipping unparseable stream-json line: ${errorMessage(parseError)}`);
        continue;
      }
      try {
        this.applyStreamEvent(event, proc);
      } catch (error) {
        proc.kill();
        throw error;
      }
    }
  }

  /**
   * Advance the in-flight stream state by one event and forward the outputs it implies.
   * The state is re-read from `this.stream` on every event, so claims and forgets made
   * between events survive, and it is committed BEFORE forwarding, so a host answering a
   * tool_call synchronously finds its tool_use.
   */
  private applyStreamEvent(event: unknown, proc: ChildProcess): void {
    if (this.stream === null) throw new Error('stream event received with no in-flight invocation state');
    const step = mapStreamEvent(event, this.stream);
    this.stream = step.state;
    for (const msg of step.outputs) {
      if (msg.type === 'notice') {
        this.emitNotice(msg);
      } else {
        this.onOutput(msg);
      }
    }
    if (step.finished !== undefined) this.finishTurn(step.finished.sessionId, proc);
  }

  /**
   * Record the CLI session id for --resume and close stdin: no tool_result follows a
   * result event, and the CLI in stream-json input mode exits only once stdin closes.
   * currentStdin is nulled at once so a late tool_result hits the "no in-flight" guard
   * rather than writing to an ended stream.
   */
  private finishTurn(sessionId: string, proc: ChildProcess): void {
    this.sessionId = sessionId;
    proc.stdin?.end();
    this.currentStdin = null;
  }

  /**
   * The MCP config file handed to the claude CLI, written on first use and cached for
   * the session lifetime (cleaned up in destroy()) so later turns skip the
   * mkdtemp+writeFile I/O. The reify-debug entry is present only when the host's server
   * is live, and the reify-permission entry only when a permission server is configured.
   */
  private mcpConfigPath(): string {
    if (!this.mcpConfigTmpFile) {
      this.mcpConfigTmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'reify-mcp-'));
      this.mcpConfigTmpFile = path.join(this.mcpConfigTmpDir, 'mcp-config.json');
      const mcpConfig = {
        mcpServers: sidecarMcpServers({
          reifyDebugMcpUrl: this.config.reifyDebugMcpUrl,
          permissionUrl: this.config.permissionMcp?.url,
        }),
      };
      fs.writeFileSync(this.mcpConfigTmpFile, JSON.stringify(mcpConfig));
    }
    return this.mcpConfigTmpFile;
  }

  /**
   * The command that runs `claude <args>`: wrapped in the landlock sandbox when the
   * startup probe (config.landlockAvailable, settled by probeLandlockAsync() in index.ts)
   * says the kernel supports it, otherwise a direct spawn. When a sandbox was requested
   * but is unavailable, emits a one-shot sandbox_unavailable notice for invocation `id`
   * so the frontend can surface a toast once per session rather than every turn.
   */
  private sandboxedCommand(id: string, args: string[]): { cmd: string; args: string[] } {
    const landlockOk = this.config.landlockAvailable ?? false;
    if (this.config.landlockExec && !landlockOk && !this.sandboxNoticeEmitted) {
      this.sandboxNoticeEmitted = true;
      this.emitNotice({
        type: 'notice',
        id,
        code: 'sandbox_unavailable',
        message: 'Sandbox unavailable; Claude will run unrestricted.',
      });
    }
    const effectiveLandlockExec = landlockOk ? this.config.landlockExec : undefined;
    const workspaceDir = this.config.workspace ?? this.config.workingDirectory;
    return wrapClaudeArgs(args, workspaceDir, effectiveLandlockExec);
  }

  /**
   * The one rule for notices: every notice goes to the host AND to stderr (the GUI
   * process's log, which the sidecar inherits), so a degraded mode stays diagnosable
   * when the chat never renders it.
   */
  private emitNotice(notice: NoticeMessage): void {
    console.warn(`[sidecar] notice ${notice.code}: ${notice.message}`);
    this.onOutput(notice);
  }

  /**
   * Emit the appropriate message after invokeSdk completes or is aborted.
   * Timeout aborts produce an error; user aborts and normal completions produce done.
   */
  private emitAbortOrDone(id: string): void {
    if (this.abortController?.signal.aborted && this.abortController.signal.reason === 'timeout') {
      const ms = this.config.timeoutMs ?? 300_000;
      this.onOutput({ type: 'error', id, message: `Claude CLI timed out after ${ms}ms` });
    } else {
      this.onOutput({ type: 'done', id });
    }
  }

  /**
   * Abort the current in-flight SDK request.
   */
  private handleAbort(): void {
    if (this.abortController) {
      this.abortController.abort();
    }
  }

  /**
   * Clear session state and emit ready.
   */
  private handleClearSession(): void {
    this.sessionId = null;
    if (this.stream !== null) this.stream = forgetToolUses(this.stream);
    // Cancel any in-flight permission requests before clearing the tracking map.
    // Without this, the HTTP handlers awaiting pendingPromises in the permission
    // server would hang forever (no one calls decide() after the map is cleared).
    this.config.permissionMcp?.server.cancelAll();
    this.pendingPermissionRequests.clear();
    this.onOutput({ type: 'ready' });
  }

  /**
   * Handle an inbound permission_decision: forward it to the permission server's decide()
   * and, when remember is true, also call setRemembered() with the associated tool_name.
   * Emits a structured error outbound when no permission server is configured or the
   * request_id is unknown.
   */
  private handlePermissionDecision(
    requestId: string,
    behavior: 'allow' | 'deny',
    message?: string,
    updatedInput?: Record<string, unknown>,
    remember?: boolean,
  ): void {
    const permServer = this.config.permissionMcp?.server;
    if (!permServer) {
      this.onOutput({
        type: 'error',
        id: this.currentInvocationId ?? '',
        message: 'permission_decision received but no permission server is configured',
      });
      return;
    }

    // Look up the tool_name for this request_id (needed for setRemembered).
    const toolName = this.pendingPermissionRequests.get(requestId);

    // Build the decision object — omit undefined optional fields.
    const decision: { behavior: 'allow' | 'deny'; message?: string; updatedInput?: Record<string, unknown> } = { behavior };
    if (message !== undefined) decision.message = message;
    if (updatedInput !== undefined) decision.updatedInput = updatedInput;

    permServer.decide(requestId, decision);

    if (remember && toolName !== undefined) {
      permServer.setRemembered(toolName);
    }

    // Clean up the tracking entry (decide() on the server already consumed the promise,
    // keeping the map entry only wastes memory on long sessions).
    if (toolName !== undefined) {
      this.pendingPermissionRequests.delete(requestId);
    }
  }
}
