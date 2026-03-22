// --- Inbound messages (GUI -> Sidecar) ---

export interface SendMessage {
  type: 'send_message';
  id: string;
  text: string;
  context?: {
    selected_entity?: string;
    diagnostics?: string[];
    constraints?: string[];
  };
}

export interface Abort {
  type: 'abort';
}

export interface ClearSession {
  type: 'clear_session';
}

export type InboundMessage = SendMessage | Abort | ClearSession;

// --- Outbound messages (Sidecar -> GUI) ---

export interface TextDelta {
  type: 'text_delta';
  text: string;
}

export interface ThinkingDelta {
  type: 'thinking_delta';
  text: string;
}

export interface ToolCall {
  type: 'tool_call';
  tool: string;
  args: Record<string, unknown>;
}

export interface ToolResult {
  type: 'tool_result';
  tool: string;
  result: string;
}

export interface Done {
  type: 'done';
}

export interface ErrorMessage {
  type: 'error';
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
