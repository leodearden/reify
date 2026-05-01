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
