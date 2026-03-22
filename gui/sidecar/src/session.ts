import { spawn } from 'node:child_process';
import type { InboundMessage, OutboundMessage } from './types.js';

export interface SessionConfig {
  model: string;
  workingDirectory: string;
  systemPrompt: string;
}

interface ConversationEntry {
  role: 'user' | 'assistant';
  content: string;
}

/**
 * Manages a Claude Code SDK session, dispatching inbound messages
 * and emitting outbound messages via the onOutput callback.
 */
export class SidecarSession {
  private config: SessionConfig;
  private conversationHistory: ConversationEntry[] = [];
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
   * Process a user message through the Claude Code SDK.
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

    // Add to conversation history
    this.conversationHistory.push({ role: 'user', content: prompt });

    // Create abort controller for this request
    this.abortController = new AbortController();

    try {
      const response = await this.invokeSdk(prompt);

      // Emit the response as text delta
      if (response && !this.abortController.signal.aborted) {
        this.onOutput({ type: 'text_delta', text: response });
        this.conversationHistory.push({ role: 'assistant', content: response });
      }

      // Emit done
      this.onOutput({ type: 'done' });
    } catch (error: unknown) {
      if (this.abortController?.signal.aborted) {
        // Aborted — don't emit error, just done
        this.onOutput({ type: 'done' });
      } else {
        const message = error instanceof Error ? error.message : String(error);
        this.onOutput({ type: 'error', message });
      }
    } finally {
      this.abortController = null;
    }
  }

  /**
   * Invoke the Claude Code SDK with the given prompt.
   * This is separated for testability (can be mocked in tests).
   */
  private async invokeSdk(prompt: string): Promise<string> {
    return new Promise<string>((resolve, reject) => {
      const args = [
        '--print',
        '--model', this.config.model,
        '--system-prompt', this.config.systemPrompt,
        prompt,
      ];

      // Add conversation history as previous messages
      if (this.conversationHistory.length > 1) {
        // The last entry is the current message, skip it
        const previousMessages = this.conversationHistory.slice(0, -1);
        args.push('--resume', JSON.stringify(previousMessages));
      }

      const proc = spawn('claude', args, {
        cwd: this.config.workingDirectory,
        signal: this.abortController?.signal,
        stdio: ['pipe', 'pipe', 'pipe'],
      });

      let stdout = '';
      let stderr = '';

      proc.stdout?.on('data', (data: Buffer) => {
        stdout += data.toString();
      });

      proc.stderr?.on('data', (data: Buffer) => {
        stderr += data.toString();
      });

      proc.on('close', (code: number | null) => {
        if (code === 0) {
          resolve(stdout.trim());
        } else {
          reject(new Error(stderr || `Claude CLI exited with code ${code}`));
        }
      });

      proc.on('error', (err: Error) => {
        reject(err);
      });
    });
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
   * Clear conversation history and emit ready.
   */
  private handleClearSession(): void {
    this.conversationHistory = [];
    this.onOutput({ type: 'ready' });
  }
}
