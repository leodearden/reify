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
   * Required by the wire contract — Rust `InboundMessage::ToolResult.tool_use_id`
   * is `String` (mandatory). The sidecar uses id-based correlation via this echoed
   * value (preferred, correct for out-of-order results). Hosts that pre-date the
   * echoed-id contract (sending without this field) will be rejected at parse time;
   * the sidecar still implements a FIFO-by-tool_name fallback at the private handler
   * level as defense-in-depth, but it is no longer reachable from the public API
   * surface. */
  tool_use_id: string;
  tool_name: string;
  result: unknown;
}

export interface PermissionDecision {
  type: 'permission_decision';
  request_id: string;
  behavior: 'allow' | 'deny';
  message?: string;
  updated_input?: Record<string, unknown>;
  remember?: boolean;
}

export type InboundMessage = SendMessage | Abort | ClearSession | InboundToolResult | PermissionDecision;

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

export interface NoticeMessage {
  type: 'notice';
  /** The in-flight `send_message` id when a turn is in flight at the time the notice
   * is emitted — correlates the notice to that turn for host-side routing. The empty
   * string is reserved for notices emitted outside any in-flight invocation (currently
   * only `permission_request_orphaned`, which fires from a permission-server callback
   * that races with destroy or occurs before the first send). */
  id: string;
  /** Stable, structured discriminator for the notice. e.g. 'degraded_turn_boundary'.
   * Hosts SHOULD route on `code` rather than substring-matching `message` prose. */
  code: string;
  message: string;
}

export interface Ready {
  type: 'ready';
}

export interface PermissionRequest {
  type: 'permission_request';
  id: string;
  request_id: string;
  tool_name: string;
  tool_input: Record<string, unknown>;
}

export type OutboundMessage =
  | TextDelta
  | ThinkingDelta
  | ToolCall
  | ToolResult
  | Done
  | ErrorMessage
  | NoticeMessage
  | Ready
  | PermissionRequest;
