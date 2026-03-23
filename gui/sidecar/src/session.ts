import { spawn } from 'node:child_process';
import { createLineReader } from './ipc.js';
import type { InboundMessage, OutboundMessage } from './types.js';

export interface SessionConfig {
  model: string;
  workingDirectory: string;
  systemPrompt: string;
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
   * Dispatch an inbound message to the appropriate handler.
   */
  async handleMessage(msg: InboundMessage): Promise<void> {
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
    }
  }

  /**
   * Process a user message through the Claude Code CLI in streaming mode.
   */
  private async handleSendMessage(
    id: string,
    text: string,
    context?: { selected_entity?: string; diagnostics?: string[]; constraints?: string[] }
  ): Promise<void> {
    // Build the prompt with optional context
    let prompt = text;
    if (context) {
      const contextParts: string[] = [];
      if (context.selected_entity) {
        contextParts.push(`Selected entity: ${context.selected_entity}`);
      }
      if (context.diagnostics?.length) {
        contextParts.push(`Diagnostics:\n${context.diagnostics.join('\n')}`);
      }
      if (context.constraints?.length) {
        contextParts.push(`Constraints:\n${context.constraints.join('\n')}`);
      }
      if (contextParts.length > 0) {
        prompt = `${text}\n\n[Context]\n${contextParts.join('\n\n')}`;
      }
    }

    // Create abort controller for this request
    this.abortController = new AbortController();

    try {
      await this.invokeSdk(id, prompt);
      this.onOutput({ type: 'done', id });
    } catch (error: unknown) {
      if (this.abortController?.signal.aborted) {
        // Aborted — don't emit error, just done
        this.onOutput({ type: 'done', id });
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
    const args = [
      '--print',
      '--output-format', 'stream-json',
      '--include-partial-messages',
      '--model', this.config.model,
      '--system-prompt', this.config.systemPrompt,
    ];

    if (this.sessionId) {
      args.push('--resume', this.sessionId);
    }

    args.push('--', prompt);

    const proc = spawn('claude', args, {
      cwd: this.config.workingDirectory,
      signal: this.abortController?.signal,
      stdio: ['pipe', 'pipe', 'pipe'],
    });

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
    const seenToolIds = new Set<string>();

    // Parse streaming JSON events from stdout
    for await (const line of createLineReader(proc.stdout!)) {
      try {
        const event = JSON.parse(line);

        if (event.type === 'assistant' && event.message?.content) {
          for (const block of event.message.content) {
            if (block.type === 'text' && block.text && block.text.length > lastTextLen) {
              const delta = block.text.slice(lastTextLen);
              lastTextLen = block.text.length;
              this.onOutput({ type: 'text_delta', id, content: delta });
            } else if (block.type === 'thinking' && block.thinking && block.thinking.length > lastThinkingLen) {
              const delta = block.thinking.slice(lastThinkingLen);
              lastThinkingLen = block.thinking.length;
              this.onOutput({ type: 'thinking_delta', id, content: delta });
            } else if (block.type === 'tool_use' && block.id && !seenToolIds.has(block.id)) {
              seenToolIds.add(block.id);
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
                tool_name: block.tool_use_id ?? '',
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

    // Wait for process exit and check exit code
    const exitCode = await exitPromise;
    if (exitCode !== 0 && exitCode !== null) {
      throw new Error(stderr || `Claude CLI exited with code ${exitCode}`);
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
    this.onOutput({ type: 'ready' });
  }
}
