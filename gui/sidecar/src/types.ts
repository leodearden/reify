// --- Inbound messages (GUI -> Sidecar) ---

export interface SendMessage {
  type: 'send_message';
  id: string;
  text: string;
  context?: {
    selected_entity?: string;
    diagnostics?: string[];
    constraints?: string[];
    current_file?: string;
    attached_contexts?: string[];
  };
}

export interface Abort {
  type: 'abort';
}

export interface ClearSession {
  type: 'clear_session';
}

export interface InboundToolResult {
  type: 'tool_result';
  id: string;
  /** The Claude CLI tool_use_id from the corresponding tool_call outbound message.
   * When present the sidecar uses id-based correlation (preferred, correct for
   * out-of-order results). When absent it falls back to FIFO-by-tool_name. */
  tool_use_id?: string;
  tool_name: string;
  result: unknown;
}

export type InboundMessage = SendMessage | Abort | ClearSession | InboundToolResult;

// --- Outbound messages (Sidecar -> GUI) ---

export interface TextDelta {
  type: 'text_delta';
  id: string;
  content: string;
}

export interface ThinkingDelta {
  type: 'thinking_delta';
  id: string;
  content: string;
}

export interface ToolCall {
  type: 'tool_call';
  id: string;
  /** The Claude CLI tool_use_id. The host should echo this back as InboundToolResult.tool_use_id
   * to enable id-based correlation and avoid the FIFO-by-tool_name in-order-only contract. */
  tool_use_id: string;
  tool_name: string;
  tool_input: Record<string, unknown>;
}

export interface ToolResult {
  type: 'tool_result';
  id: string;
  tool_name: string;
  result: unknown;
}

export interface Done {
  type: 'done';
  id: string;
}

export interface ErrorMessage {
  type: 'error';
  id: string;
  message: string;
}

export interface Ready {
  type: 'ready';
}

export type OutboundMessage =
  | TextDelta
  | ThinkingDelta
  | ToolCall
  | ToolResult
  | Done
  | ErrorMessage
  | Ready;
