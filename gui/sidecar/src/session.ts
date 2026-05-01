import { spawn } from 'node:child_process';
import { createLineReader } from './ipc.js';
import type { InboundMessage, OutboundMessage, SendMessage } from './types.js';

export interface SessionConfig {
  model: string;
  workingDirectory: string;
  systemPrompt: string;
  /** Timeout in milliseconds for each SDK invocation. Default: 300_000 (5 minutes). */
  timeoutMs?: number;
}

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

  /** Called when the session produces an outbound message. */
  onOutput: (msg: OutboundMessage) => void = () => {};

  constructor(config: SessionConfig) {
    this.config = config;
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
    this.sessionId = null;
    this.toolNameById.clear();
    this.pendingToolUseIds.clear();
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
        this.handleToolResult(msg.tool_name, msg.result, msg.id);
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
      '--output-format', 'stream-json',
      '--include-partial-messages',
      '--input-format', 'stream-json',
      '--model', this.config.model,
      '--system-prompt', this.config.systemPrompt,
    ];

    if (this.sessionId) {
      args.push('--resume', this.sessionId);
    }

    const proc = spawn('claude', args, {
      cwd: this.config.workingDirectory,
      signal: this.abortController?.signal,
      stdio: ['pipe', 'pipe', 'pipe'],
    });

    // Write the initial user prompt as a stream-json message line.
    // Keep stdin open (do NOT call end()) so tool_result blocks can follow.
    proc.stdin?.write(
      JSON.stringify({
        type: 'user',
        message: { role: 'user', content: [{ type: 'text', text: prompt }] },
      }) + '\n'
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

    // Parse streaming JSON events from stdout
    try {
      for await (const line of createLineReader(proc.stdout!)) {
        try {
          const event = JSON.parse(line);

          if (event.type === 'assistant' && event.message?.content) {
            for (const block of event.message.content) {
              // Detect new turn: if text/thinking length decreased, counters
              // from the previous turn carry over — reset them so deltas emit correctly.
              if (block.type === 'text' && block.text) {
                if (block.text.length < lastTextLen) {
                  lastTextLen = 0;
                  lastThinkingLen = 0;
                  this.toolNameById.clear();
                  this.pendingToolUseIds.clear();
                }
                if (block.text.length > lastTextLen) {
                  const delta = block.text.slice(lastTextLen);
                  lastTextLen = block.text.length;
                  this.onOutput({ type: 'text_delta', id, content: delta });
                }
              } else if (block.type === 'thinking' && block.thinking) {
                if (block.thinking.length < lastThinkingLen) {
                  lastTextLen = 0;
                  lastThinkingLen = 0;
                  this.toolNameById.clear();
                  this.pendingToolUseIds.clear();
                }
                if (block.thinking.length > lastThinkingLen) {
                  const delta = block.thinking.slice(lastThinkingLen);
                  lastThinkingLen = block.thinking.length;
                  this.onOutput({ type: 'thinking_delta', id, content: delta });
                }
              } else if (block.type === 'tool_use' && block.id && !this.toolNameById.has(block.id)) {
                this.toolNameById.set(block.id, block.name);
                // Push to FIFO queue so tool_result inbound can pop the matching id
                const queue = this.pendingToolUseIds.get(block.name) ?? [];
                queue.push(block.id);
                this.pendingToolUseIds.set(block.name, queue);
                this.onOutput({
                  type: 'tool_call',
                  id,
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
          }
        } catch {
          // Skip unparseable lines
        }
      }
    } finally {
      clearTimeout(timeoutId);
      this.currentStdin = null;
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
  private handleToolResult(toolName: string, result: unknown, id: string): void {
    // Validate currentStdin BEFORE consuming the FIFO queue. If we shifted first and
    // then found currentStdin is null, we'd silently drain the queue and corrupt
    // subsequent matching results.
    if (this.currentStdin === null) {
      this.onOutput({
        type: 'error',
        id,
        message: `no pending tool_use for tool_name=${toolName}`,
      });
      return;
    }
    const queue = this.pendingToolUseIds.get(toolName);
    const toolUseId = queue?.shift();
    if (toolUseId === undefined) {
      this.onOutput({
        type: 'error',
        id,
        message: `no pending tool_use for tool_name=${toolName}`,
      });
      return;
    }
    this.currentStdin.write(
      JSON.stringify({
        type: 'user',
        message: {
          role: 'user',
          content: [{ type: 'tool_result', tool_use_id: toolUseId, content: result }],
        },
      }) + '\n'
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
    this.onOutput({ type: 'ready' });
  }
}
