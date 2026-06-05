import { spawn } from 'node:child_process';
import * as fs from 'node:fs';
import * as path from 'node:path';
import * as os from 'node:os';
import { createLineReader } from './ipc.js';
import type { InboundMessage, OutboundMessage, SendMessage } from './types.js';
import type { PermissionServer } from './permission-server.js';
import { wrapClaudeArgs } from './sandbox.js';

export interface SessionConfig {
  model: string;
  workingDirectory: string;
  systemPrompt: string;
  /** Timeout in milliseconds for each SDK invocation. Default: 300_000 (5 minutes). */
  timeoutMs?: number;
  /**
   * Optional permission-prompt MCP server to wire Claude CLI's --permission-prompt-tool.
   * When provided, invokeSdk appends --mcp-config and --permission-prompt-tool args.
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
}

/**
 * Diagnostic log prefix used by the proc.on('error') handler for non-ABORT spawn errors.
 * Exported so tests can reference it without hardcoding the string literal.
 */
export const SPAWN_ERROR_LOG_PREFIX = '[sidecar] spawned claude error:';

/**
 * Resolve the reify-debug MCP endpoint URL from the environment.
 *
 * Accepts only pure decimal digit strings (no whitespace, no trailing chars),
 * matching the Rust `parse_debug_port` contract in `debug_server.rs`.
 * Falls back to port 3939 for unset / empty / non-digit / out-of-range input.
 *
 * Cross-ref: `gui/test/visual/endpoint.ts` `resolveDebugPort` uses identical
 * validation logic; `gui/src-tauri/src/debug_server.rs` `parse_debug_port` is
 * the Rust source-of-truth.  Keep all three in lockstep if rules change.
 */
function resolveReifyDebugUrl(env: NodeJS.ProcessEnv = process.env): string {
  const raw = env['REIFY_DEBUG_PORT'];
  // Strict digits-only — rejects whitespace-padded (" 4500 ") and trailing
  // garbage ("4500x") that parseInt would silently accept.
  const n = raw !== undefined && /^\d+$/.test(raw) ? parseInt(raw, 10) : NaN;
  const port = Number.isFinite(n) && n >= 1 && n <= 65535 ? n : 3939;
  return `http://127.0.0.1:${port}/mcp`;
}

/**
 * Tools Claude is allowed to use without per-call permission prompts.
 * Covers standard FS/shell tools plus all reify-debug MCP tools.
 * Using bypassPermissions+allowedTools is strictly tighter than
 * --dangerously-skip-permissions (which would allow everything).
 */
export const ALLOWED_TOOLS = 'Read Edit Write Bash Glob Grep mcp__reify-debug__*';

/**
 * Manages a Claude Code SDK session, dispatching inbound messages
 * and emitting outbound messages via the onOutput callback.
 *
 * Uses `claude --print --output-format stream-json` for streaming
 * responses, and `--resume <session_id>` for conversation continuity.
 */
export class SidecarSession {
  private config: SessionConfig;
  private sessionId: string | null = null;
  private abortController: AbortController | null = null;
  private destroyed = false;

  /** Maps tool_use_id → tool_name for outbound tool_result rendering. */
  private toolNameById: Map<string, string> = new Map();
  /** Maps tool_name → FIFO queue of pending tool_use_ids for correlation. */
  private pendingToolUseIds: Map<string, string[]> = new Map();
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
          this.onOutput({
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
    this.toolNameById.clear();
    this.pendingToolUseIds.clear();
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
      case 'tool_result':
        this.handleToolResult(msg.tool_name, msg.result, msg.id, msg.tool_use_id);
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
    // Clear correlation state from any prior invocation before building args.
    // This guarantees a fresh start regardless of how the previous invocation ended
    // (normal exit, error, abort, or stream-error path).
    this.toolNameById.clear();
    this.pendingToolUseIds.clear();

    const args = [
      '--print',
      '--verbose',
      '--output-format', 'stream-json',
      '--include-partial-messages',
      '--input-format', 'stream-json',
      '--model', this.config.model,
      '--system-prompt', this.config.systemPrompt,
    ];

    if (this.sessionId) {
      args.push('--resume', this.sessionId);
    }

    // Write the MCP config file lazily on the first invokeSdk call and cache it for the
    // session lifetime (mcpConfigTmpFile / mcpConfigTmpDir fields), so subsequent turns
    // avoid repeated mkdtemp+writeFile I/O. Cleanup happens in destroy().
    // The config always includes the reify-debug entry (for GUI tooling); the
    // reify-permission entry is added only when a permission server is configured.
    if (!this.mcpConfigTmpFile) {
      this.mcpConfigTmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'reify-mcp-'));
      this.mcpConfigTmpFile = path.join(this.mcpConfigTmpDir, 'mcp-config.json');
      const mcpConfig = {
        mcpServers: {
          'reify-debug': { type: 'http', url: resolveReifyDebugUrl() },
          ...(this.config.permissionMcp
            ? { 'reify-permission': { type: 'http', url: this.config.permissionMcp.url } }
            : {}),
        },
      };
      fs.writeFileSync(this.mcpConfigTmpFile, JSON.stringify(mcpConfig));
    }
    args.push('--mcp-config', this.mcpConfigTmpFile);
    if (this.config.permissionMcp) {
      args.push('--permission-prompt-tool', 'mcp__reify-permission__approve_tool');
    }

    // Always bypass per-call permission prompts using an explicit allowlist.
    // This prevents the GUI from silently stalling on missing permission-UI (see task #3206).
    // The allowlist is strictly tighter than --dangerously-skip-permissions (which allows all tools).
    //
    // Precedence note: --permission-mode bypassPermissions auto-approves ALL tool calls without
    // asking — so --permission-prompt-tool (reify-permission) is NOT consulted for any tool in
    // bypassPermissions mode. The two flags are complementary in purpose but operate on different
    // axes: --allowed-tools restricts which tools Claude is permitted to invoke (anything outside
    // the list is rejected outright); --permission-mode bypassPermissions means approved tool
    // calls need no further confirmation prompt. The permission MCP (reify-permission) would only
    // become active if a future configuration uses a non-bypass permission mode.
    args.push('--permission-mode', 'bypassPermissions');
    args.push('--allowed-tools', ALLOWED_TOOLS);

    // Read the landlock probe result that was settled at sidecar startup by
    // probeLandlockAsync() in index.ts. Defaults to false when the field is absent
    // (e.g. unit-test invocations that do not set landlockAvailable in config).
    const landlockOk = this.config.landlockAvailable ?? false;

    // When a sandbox was requested (landlockExec set) but the kernel can't deliver it,
    // emit a one-shot notice so the frontend can surface a toast. The !sandboxNoticeEmitted
    // guard prevents spamming the notice on every subsequent turn of the same session.
    if (this.config.landlockExec && !landlockOk && !this.sandboxNoticeEmitted) {
      this.sandboxNoticeEmitted = true;
      this.onOutput({
        type: 'notice',
        id,
        code: 'sandbox_unavailable',
        message: 'Sandbox unavailable; Claude will run unrestricted.',
      });
      console.warn('[sidecar] sandbox unavailable; claude will run unrestricted');
    }

    // Wrap the claude args with the landlock sandbox when available.
    // effectiveLandlockExec is set to undefined if the probe failed so wrapClaudeArgs
    // returns {cmd:'claude', args:[...args]} (passthrough, no python3 wrap).
    const effectiveLandlockExec = landlockOk ? this.config.landlockExec : undefined;
    const workspaceDir = this.config.workspace ?? this.config.workingDirectory;
    const { cmd, args: wrappedArgs } = wrapClaudeArgs(args, workspaceDir, effectiveLandlockExec);

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
      // One-shot guard (mirrors notifiedMissingMessageId below) prevents log spam if the
      // kernel/Node fires repeated errors on the same stream after an external fd close.
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

    // Register the in-flight stdin so handleToolResult can write to it.
    this.currentStdin = proc.stdin ?? null;

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

    // Track content lengths for delta extraction from partial messages
    let lastTextLen = 0;
    let lastThinkingLen = 0;
    let currentAssistantMessageId: string | null = null;
    // One-shot guard: notify (stderr + host via onOutput) at most once per invocation if message.id is absent.
    let notifiedMissingMessageId = false;

    // Parse streaming JSON events from stdout
    try {
      for await (const line of createLineReader(proc.stdout!)) {
        try {
          const event = JSON.parse(line);

          if (event.type === 'assistant' && event.message?.content) {
            // Detect new turn from the streaming envelope's message.id. Each new
            // assistant turn within a single --print invocation gets a new Message.id;
            // partial updates within the same turn share the same id. Reset both length
            // counters deterministically on id change so deltas emit from offset 0 for
            // the new turn. If event.message.id is absent (malformed mock or future
            // format change), skip the reset — degrading gracefully to single-turn
            // behavior. Tool-correlation maps (toolNameById, pendingToolUseIds) are NOT
            // cleared here — pending tool_uses from a prior turn may still be awaiting
            // a tool_result from the host. Maps are only cleared at the three lifecycle
            // boundaries: invokeSdk start, destroy(), and handleClearSession.
            if (typeof event.message.id === 'string' && event.message.id !== currentAssistantMessageId) {
              lastTextLen = 0;
              lastThinkingLen = 0;
              currentAssistantMessageId = event.message.id;
            } else if (typeof event.message.id !== 'string' && !notifiedMissingMessageId) {
              notifiedMissingMessageId = true;
              const DEGRADED_TURN_BOUNDARY_DETAIL =
                'assistant event missing message.id — turn-boundary detection disabled for this invocation; ' +
                'multi-turn delta offsets may be incorrect if accumulated text length is not reset between turns.';
              console.error(`[sidecar] ${DEGRADED_TURN_BOUNDARY_DETAIL}`);
              this.onOutput({
                type: 'notice',
                id,
                code: 'degraded_turn_boundary',
                message: DEGRADED_TURN_BOUNDARY_DETAIL,
              });
            }
            for (const block of event.message.content) {
              if (block.type === 'text' && block.text) {
                if (block.text.length > lastTextLen) {
                  const delta = block.text.slice(lastTextLen);
                  lastTextLen = block.text.length;
                  this.onOutput({ type: 'text_delta', id, content: delta });
                }
              } else if (block.type === 'thinking' && block.thinking) {
                if (block.thinking.length > lastThinkingLen) {
                  const delta = block.thinking.slice(lastThinkingLen);
                  lastThinkingLen = block.thinking.length;
                  this.onOutput({ type: 'thinking_delta', id, content: delta });
                }
              } else if (block.type === 'tool_use' && block.id && !this.toolNameById.has(block.id)) {
                this.toolNameById.set(block.id, block.name);
                // Maintain a FIFO queue of tool_use_ids per tool_name as a fallback
                // correlation mechanism for hosts that do not echo back tool_use_id.
                // CONTRACT: the fallback assumes the host returns tool_results in the
                // same order Claude CLI emitted tool_uses for the same tool_name;
                // out-of-order same-name results will be mis-correlated. Hosts should
                // echo tool_use_id (included in the tool_call outbound below) in their
                // InboundToolResult to enable correct id-based correlation instead.
                const queue = this.pendingToolUseIds.get(block.name) ?? [];
                queue.push(block.id);
                this.pendingToolUseIds.set(block.name, queue);
                this.onOutput({
                  type: 'tool_call',
                  id,
                  tool_use_id: block.id,
                  tool_name: block.name,
                  tool_input: block.input ?? {},
                });
              } else if (block.type === 'tool_result') {
                this.onOutput({
                  type: 'tool_result',
                  id,
                  tool_name: this.toolNameById.get(block.tool_use_id) ?? '',
                  result: block.content,
                });
              }
            }
          } else if (event.type === 'result' && event.session_id) {
            this.sessionId = event.session_id;
            // Close stdin now that the invocation is complete. No further tool_results
            // are expected after a 'result' event; keeping stdin open would prevent
            // claude CLI from exiting deterministically.
            proc.stdin?.end();
            // Null currentStdin immediately so any tool_result arriving between this
            // point and the finally block hits the clean "no in-flight" guard rather
            // than writing to an already-ended stream (Bug #2: close-on-result race).
            this.currentStdin = null;
          }
        } catch {
          // Skip unparseable lines
        }
      }
    } finally {
      clearTimeout(timeoutId);
      this.currentStdin = null;
      // MCP config file cleanup is deferred to destroy() since the file is
      // now cached for the session lifetime rather than written per-turn.
    }

    // Wait for process exit and check exit code
    const exitCode = await exitPromise;
    if (exitCode !== 0 && exitCode !== null) {
      throw new Error(stderr || `Claude CLI exited with code ${exitCode}`);
    }
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
   * Forward a tool_result inbound from the host to the in-flight claude CLI's stdin.
   * Correlates tool_name → tool_use_id using the FIFO pendingToolUseIds queue.
   * Emits an error outbound if no matching tool_use_id is found.
   */
  private handleToolResult(toolName: string, result: unknown, id: string, inboundToolUseId?: string): void {
    // Validate currentStdin BEFORE consuming the FIFO queue. If we shifted first and
    // then found currentStdin is null, we'd silently drain the queue and corrupt
    // subsequent matching results.
    if (this.currentStdin === null) {
      // Distinct message: no claude CLI process is running at all.
      this.onOutput({
        type: 'error',
        id,
        message: 'no in-flight claude CLI process',
      });
      return;
    }

    let toolUseId: string | undefined;
    if (inboundToolUseId !== undefined) {
      // Preferred path: host echoed back the tool_use_id from the tool_call outbound.
      // Validate it before accepting: the id must be registered AND match the tool_name.
      if (!this.toolNameById.has(inboundToolUseId)) {
        this.onOutput({
          type: 'error',
          id,
          message: `unknown tool_use_id=${inboundToolUseId} for tool_name=${toolName}`,
        });
        return;
      }
      const registeredName = this.toolNameById.get(inboundToolUseId);
      if (registeredName !== toolName) {
        this.onOutput({
          type: 'error',
          id,
          message: `tool_use_id=${inboundToolUseId} does not match tool_name=${toolName} (registered as ${registeredName})`,
        });
        return;
      }
      // Validation passed: use the echoed id directly — no FIFO consumed, correct for out-of-order results.
      toolUseId = inboundToolUseId;
      // Drain the matched id from the FIFO (splice, not shift, to preserve other ids at different
      // positions — out-of-order delivery between distinct ids is the reason echoed-id exists).
      const queue = this.pendingToolUseIds.get(toolName);
      if (queue) {
        const idx = queue.indexOf(inboundToolUseId);
        if (idx !== -1) queue.splice(idx, 1);
        if (queue.length === 0) this.pendingToolUseIds.delete(toolName);
      }
    } else {
      // Fallback path: FIFO queue by tool_name.
      // See the in-order-only CONTRACT documented in invokeSdk's tool_use handler.
      const queue = this.pendingToolUseIds.get(toolName);
      toolUseId = queue?.shift();
      // Clean up an emptied FIFO entry to avoid stale map entries.
      if (queue && queue.length === 0) this.pendingToolUseIds.delete(toolName);
    }

    if (toolUseId === undefined) {
      // Distinct message: a process is running but this tool_name has no queued id.
      this.onOutput({
        type: 'error',
        id,
        message: `no pending tool_use for tool_name=${toolName}`,
      });
      return;
    }
    // Remove the consumed id from toolNameById BEFORE the write so the maps stay consistent
    // regardless of write outcome — the tool_use is "consumed" once we commit to forwarding it.
    //
    // Contract: each emitted tool_use_id is valid for exactly one tool_result forward. If the
    // write fails (EPIPE reported via the per-write callback below), the host should treat the
    // original send_message as failed and retry as a new send_message rather than re-dispatching
    // the same tool_result. Retrying a tool_result with a consumed id will hit the 'unknown
    // tool_use_id' validation guard and produce a clean structured error.
    this.toolNameById.delete(toolUseId);
    // Per-write callback captures the tool_result id for correct error correlation.
    // No synchronous try/catch needed: stream-write failures surface via the callback.
    this.currentStdin.write(
      JSON.stringify({
        type: 'user',
        message: {
          role: 'user',
          content: [{ type: 'tool_result', tool_use_id: toolUseId, content: result }],
        },
      }) + '\n',
      (err) => {
        if (err) this.onOutput({ type: 'error', id, message: `stdin write error: ${err.message}` });
      }
    );
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
    this.toolNameById.clear();
    this.pendingToolUseIds.clear();
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
