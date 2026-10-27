export type ProviderId = "openai" | "anthropic" | "openrouter";

export interface OpenConfig {
  provider: ProviderId; // v1 default: "openrouter"
  modelId: string;
  prefix: string; // default "pi_"
}

export interface OpenDocumentReq {
  documentId: string;       // app UUID
  title: string;
  pageCount: number;
  conversationId?: number;  // reattach
  provider: ProviderId;
  modelId: string;
}

export interface OpenDocumentRes {
  conversationId: number;
}

export interface SubmitReq {
  conversationId: number;
  content: string;
  requestId: string;
  selection?: { page: number; quote: string; occurrence: number }; // page 1-based
}

export interface SubmitRes {
  submissionId: number;
  conversationId: number;
  status: "done" | "unanswered" | "aborted";
}

/** Structural commits only — not every token. */
export interface CommitEvent {
  conversationId: number;
  reason: "user_entry" | "assistant_entry" | "tool_result" | "other";
  /** Structural transcript only; live snapshots stay on the stream channel. */
  headEntryId?: number;
  /** Transcript snapshot on structural changes only (never per token). */
  entries?: readonly unknown[];
  followEnabled?: boolean;
  busy: boolean;
}

/** Token / tool output. The only per-token channel. */
export interface StreamEvent {
  conversationId: number;
  live: unknown; // docs["pi.live"]
}

export interface ReportEvent {
  type: string;
  message: string;
  task?: { id: number; kind: string; conversationId: number };
}

/** 1.4.0 AgentChange subset we pass to configure(). */
export interface AgentChange {
  model?: { provider: string; modelId: string } | null;
  thinkingLevel?: string;
  instructions?: string;
  cwd?: string;
}

export interface SetFollowReq {
  conversationId: number;
  enabled: boolean;
}
