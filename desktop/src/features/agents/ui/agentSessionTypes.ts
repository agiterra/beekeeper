import type { LucideIcon } from "lucide-react";

/** Who produced a coding-session turn's dollar estimate (ledger 272(d)). */
export type CodingSessionCostBasis = "adapter_estimate" | "table_estimate";

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
  /**
   * The `toolCallId` of the Task/Agent call whose subagent produced this item,
   * when a coding-session provider published one (44225 `parentToolId`,
   * ledger 308). The coding-session stream nests such items under that call
   * instead of showing them in the lead's reading order.
   */
  parentToolId?: string;
};

/**
 * What a provider reported about the subagent a Task/Agent call ran. Every
 * field is independently absent; absent means not reported, never zero.
 */
export type TranscriptSubagentReport = {
  type?: string;
  model?: string;
  totalTokens?: number;
  durationMs?: number;
  toolUseCount?: number;
};

/**
 * A tool result whose output the provider could not verify complete (44225
 * `outputComplete: false`): the text was assembled from streamed chunks, and
 * the adapter is known to drop the beginning of a command's output. Present
 * only on such a result; a complete, recovered or unlabelled result has none.
 */
export type TranscriptToolOutputGap = {
  /** Bytes captured from the stream, when the provider reported a count. */
  streamedBytes?: number;
  /** Bytes the adapter said the full output had, when it said. */
  aggregatedBytes?: number;
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
      /**
       * The `commandId` of the 44220 turn-start this `role: "user"` message
       * echoes, when the provider stamped one.
       *
       * Already projected onto coding-session items
       * (`CodingSessionProjectedTranscriptItem`); declared here because
       * attribution needs it. A command id is what separates a prompt a person
       * typed from one the app published on their key — an automatic
       * `team-wake-` command is founder-signed but nobody wrote it, and
       * `operatorPubkey` alone cannot tell those apart.
       */
      commandId?: string;
      /**
       * True for a `role: "user"` prompt the provider echoed with
       * `steered: true`: it was injected into a turn already running rather
       * than starting one. Rendered as a visible marker beside the author, so
       * a reader can tell a mid-turn correction from the prompt that opened
       * the turn. Present only when true.
       */
      steered?: boolean;
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
       * Whose estimate `costUsd` is (ledger 272(d)): the adapter's own figure,
       * or the provider's price table applied to reported tokens. Neither is
       * an invoice. `null` when the item named no basis (a record older than
       * ledger 266) — still an estimate, just an unattributed one.
       */
      costBasis?: CodingSessionCostBasis | null;
      /**
       * Per-turn token accounting, when the driver reported any.
       *
       * Mirrors the wire's `TurnUsageReport`
       * (`crates/beekeeper-core/src/coding_session_payload.rs`, `deny_unknown_fields`)
       * field for field: six optional numbers and nothing else. There is no
       * pricing identity on the wire — cost travels in `costUsd` above — so
       * nothing here may be read as one. Every field is independently absent;
       * absent is never `0`.
       */
      usage?: {
        inputTokens?: number;
        outputTokens?: number;
        cacheReadTokens?: number;
        cacheWriteTokens?: number;
        toolCalls?: number;
        contextWindow?: number;
      } | null;
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
      beekeeperToolName: string | null;
      status: ToolStatus;
      args: Record<string, unknown>;
      result: string;
      isError: boolean;
      /**
       * ACP's own tool *discriminant* (`"edit"`, `"read"`, `"execute"`, …) as
       * the producer published it, kept distinct from the display `toolName`.
       *
       * Authoritative where the name is not: claude-agent-acp calls its editor
       * `Edit` and, while the arguments are still streaming, `Preparing file…`
       * — neither of which any name rule in the classifier matches. Absent when
       * the adapter sent no discriminant; never guessed.
       */
      toolKind?: string | null;
      /**
       * Files the producer said this call touched, from ACP's `locations` and
       * diff blocks. Empty when the producer reported none — which is not the
       * same as the call touching none, and the Observed-changes surface says
       * so rather than reporting zero changes.
       */
      editPaths?: string[];
      /**
       * The producer's own id for this call (ACP `toolCallId`), when it sent
       * one. It is what a subagent's items name as their `parentToolId`.
       */
      toolCallId?: string;
      /** The result's subagent report, on a Task/Agent call that carried one. */
      subagent?: TranscriptSubagentReport;
      /** Set only when the output may be missing its beginning. */
      outputGap?: TranscriptToolOutputGap;
      timestamp: string;
      startedAt: string;
      completedAt: string | null;
      acpSource?: TranscriptAcpSource;
    } & TranscriptItemIdentity);

export type PromptSection = {
  title: string;
  body: string;
};

export type BeekeeperToolInfo = {
  icon: LucideIcon;
  label: string;
  tone: "read" | "write" | "admin";
};
