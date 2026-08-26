import type { LucideIcon } from "lucide-react";

export type ObserverEvent = {
  seq: number;
  timestamp: string;
  kind: string;
  agentIndex: number | null;
  channelId: string | null;
  sessionId: string | null;
  turnId: string | null;
  startedAt?: string | null;
  payload: unknown;
};

export type ConnectionState =
  | "idle"
  | "connecting"
  | "open"
  | "closed"
  | "error";

export type ToolStatus = "executing" | "completed" | "failed" | "pending";

export type AgentActivityRenderClass =
  | "message"
  | "relay-op"
  | "file-edit"
  | "file-read"
  | "skill-read"
  | "image"
  | "shell"
  | "status"
  | "thought"
  | "plan"
  | "permission"
  | "error"
  | "generic"
  | "raw-rail"
  | "suppressed";

export type AgentActivityTone = "read" | "write" | "admin" | "neutral";

export type AgentActivityAction = {
  verb: string;
  object?: string | null;
};

export type AgentActivityDescriptor = {
  renderClass: AgentActivityRenderClass;
  label: string;
  preview: string | null;
  action?: AgentActivityAction;
  tone?: AgentActivityTone;
  operation?: string;
  object?: string | null;
  source?: "mcp" | "shell" | "acp" | "harness" | "fallback";
  groupKey?: string;
  reason?: string;
};

/** Observer/ACP wire label for dev-only transcript debugging. */
export type TranscriptAcpSource = string;

/**
 * An item the producer dropped whole rather than publish over the event cap.
 *
 * Not a privacy redaction — different cause, and the reader is owed the
 * difference. `bytes` and `digest` are what the producer recorded about what
 * it dropped, so "the provider had this and chose not to publish it" stays
 * distinguishable from "nothing was there".
 */
export type TranscriptItemElision = {
  bytes: number | null;
  digest: string | null;
  reason: string;
};

/** Shared optional identity fields attached during transcript construction. */
export type TranscriptItemIdentity = {
  turnId?: string | null;
  sessionId?: string | null;
  channelId?: string | null;
  /**
   * The bridge identity that authored this item, when it did not come from a
   * locally observed ACP stream.
   *
   * Coding-session transcripts arrive as signed relay events from a provider,
   * so the renderer needs to say whose claim it is showing. `label` is display
   * metadata resolved from the trust allowlist; authority always comes from
   * `pubkey`.
   */
  bridgeSource?: {
    pubkey: string;
    label: string;
  } | null;
  /**
   * The **provider's own** session UUID, as it appears in the `cs-target`.
   *
   * Distinct from `sessionId`, which is a display/scope key
   * (`coding-session-transcript-generation/v1:…`) built to keep items from
   * colliding across channels and targets. Anything that has to name this
   * session to the *host* — the redaction vault is keyed by it — needs this
   * one, and passing `sessionId` there silently addresses nothing.
   */
  providerSessionId?: string | null;
};

export type TranscriptItem =
  | ({
      id: string;
      type: "message";
      renderClass: "message";
      role: "assistant" | "user";
      title: string;
      text: string;
      timestamp: string;
      messageId?: string | null;
      acpSource?: TranscriptAcpSource;
      authorPubkey?: string | null;
      /**
       * The operator whose verified command drove this turn, for `role: "user"`
       * messages that carry the attribution.
       *
       * Coding sessions are multi-operator — a founder plus granted operators
       * can each steer the same execution — so a user message is not
       * necessarily the viewer's own. Absent on items published before the
       * provider stamped attribution, which stay unattributed rather than
       * being assigned to anyone.
       */
      operatorPubkey?: string | null;
    } & TranscriptItemIdentity)
  | ({
      id: string;
      type: "thought";
      renderClass: "thought";
      title: string;
      text: string;
      timestamp: string;
      acpSource?: TranscriptAcpSource;
    } & TranscriptItemIdentity)
  | ({
      id: string;
      type: "plan";
      renderClass: "plan";
      title: string;
      text: string;
      timestamp: string;
      isUpdate?: boolean;
      targetId?: string;
      acpSource?: TranscriptAcpSource;
    } & TranscriptItemIdentity)
  | ({
      id: string;
      type: "lifecycle";
      renderClass: "status" | "permission" | "error";
      title: string;
      text: string;
      /** Resolved outcome for permission items (e.g. "Approved (allow_once)", "Denied (reject_once)", "Cancelled"). */
      outcome?: string;
      /** Structured turn duration for coding-session "Turn result" items. */
      durationMs?: number | null;
      /** Structured turn cost (USD) for coding-session "Turn result" items. */
      costUsd?: number | null;
      /**
       * Present when this row stands in for an item the producer dropped whole
       * because it exceeded the event cap. Structured rather than formatted
       * into `text` so the renderer can show it in the same pill vocabulary as
       * a privacy redaction while still naming the different cause.
       */
      elision?: TranscriptItemElision;
      timestamp: string;
      descriptor?: AgentActivityDescriptor;
      acpSource?: TranscriptAcpSource;
    } & TranscriptItemIdentity)
  | ({
      id: string;
      type: "metadata";
      renderClass: "raw-rail";
      title: string;
      sections: PromptSection[];
      timestamp: string;
      acpSource?: TranscriptAcpSource;
    } & TranscriptItemIdentity)
  | ({
      id: string;
      type: "tool";
      renderClass: AgentActivityRenderClass;
      descriptor: AgentActivityDescriptor;
      title: string;
      toolName: string;
      buzzToolName: string | null;
      status: ToolStatus;
      args: Record<string, unknown>;
      result: string;
      isError: boolean;
      timestamp: string;
      startedAt: string;
      completedAt: string | null;
      acpSource?: TranscriptAcpSource;
    } & TranscriptItemIdentity);

export type PromptSection = {
  title: string;
  body: string;
};

export type BuzzToolInfo = {
  icon: LucideIcon;
  label: string;
  tone: "read" | "write" | "admin";
};
