/**
 * The 44225 transcript-item contract, as the consumer reads it.
 *
 * The canonical producer-side shape lives in
 * `crates/buzz-session-provider/src/transcript.rs`; this is the consumer's
 * mirror of the item union carried in a CST envelope's `item` field. The union
 * stays deliberately permissive — the wire contract fixes the recognized
 * kinds, not the exact per-kind field list — so every reader below is
 * defensive and nothing is ever assumed present or well-typed.
 */

/**
 * What an `edit`-kind tool call touched, as the producer published it.
 *
 * Mirrors `buzz_core::coding_session_payload::tool_edit_payload`. Every field
 * optional and every reader defensive: an absent `paths` means the producer
 * reported no file, which is not the same as the call touching none.
 */
export type CodingSessionToolEditPayloadV1 = {
  paths?: string[];
  changes?: {
    path?: string;
    oldText?: string;
    newText?: string;
    truncated?: boolean;
  }[];
  truncated?: boolean;
};

/**
 * A `kind`-discriminated classification of the transcript entry union, or a
 * quarantine record. Unknown kinds are legal and must degrade, never throw.
 */
export type CodingSessionTranscriptItemV1 =
  | CodingSessionKnownTranscriptItemV1
  | CodingSessionQuarantineItemV1
  | { kind: string; [key: string]: unknown };

export type CodingSessionKnownTranscriptItemV1 = { [key: string]: unknown } & (
  | {
      kind: "user_prompt";
      content?: string;
      steered?: boolean;
      attachmentCount?: number;
      /**
       * The `commandId` of the 44220 `thread.turn.start` that started this
       * turn — the join between what an operator sent and what the provider
       * echoed back.
       *
       * Additive and genuinely optional. A provider from before this contract
       * omits it, and so does every item that no command started; for the
       * initial turn embedded in a 44221 create the provider stamps the
       * *create's* commandId, so every operator-originated prompt is
       * joinable. Bounded exactly like a 44220 commandId: non-blank, at most
       * 256 bytes, no control characters.
       */
      commandId?: string;
      /**
       * The operator the provider verified before running the turn, as
       * 64-character lowercase hex. Additive: items published before the
       * provider stamped attribution simply omit it.
       */
      operatorPubkey?: string;
    }
  | { kind: "assistant_text"; text?: string }
  | {
      kind: "tool_call";
      /**
       * `toolKind` is ACP's optional tool *discriminant* ("read", "execute",
       * "think"), kept distinct from the display `toolName`. Additive and
       * genuinely optional: the provider omits it when the adapter sent none,
       * and items published before it existed simply lack it.
       */
      tool?: {
        toolName?: string;
        toolKind?: string;
        toolId?: string;
        input?: unknown;
        /**
         * What an `edit`-kind call touched, from ACP's `locations` and its
         * `diff` content blocks. Additive and optional; a producer from before
         * it existed omits it, and so does any call that reported neither.
         *
         * `paths` is deduplicated. Each `changes` entry carries the adapter's
         * own `oldText`/`newText`, each key absent rather than `""` when the
         * adapter sent none. Bounded: `truncated` on a change means its texts
         * were shortened, `truncated` on the payload means whole changes were
         * dropped. Truncation is never silent.
         */
        edit?: CodingSessionToolEditPayloadV1;
      };
    }
  | {
      kind: "tool_result";
      toolId?: string;
      toolName?: string;
      /** The opening call's ACP discriminant, carried onto its result. */
      toolKind?: string;
      /**
       * The arguments the adapter finished streaming after the call opened.
       *
       * Additive: claude-agent-acp opens an edit with an empty `rawInput` and
       * fills it in on a later update, so a result that omits this is a
       * producer that never re-read those frames — not a call with no
       * arguments.
       */
      input?: unknown;
      /** The same edit payload the opening call carries, as finally known. */
      edit?: CodingSessionToolEditPayloadV1;
      content?: unknown;
      isError?: boolean;
    }
  | {
      kind: "result";
      subtype?: "success" | "error" | "cancelled";
      isError?: boolean;
      durationMs?: number;
      result?: string;
      /**
       * The raw provider error when `result` has been rewritten into an
       * operator sentence (a dead login, say). Absent when `result` already
       * is the raw text.
       */
      detail?: string;
      costUsd?: number;
      /**
       * Whose estimate `costUsd` is. `adapter_estimate` / `table_estimate`
       * since ledger 272(d); records published before carry the older
       * spellings `billed` (the adapter's figure — never an invoice) and
       * `estimated` (the price table's).
       */
      costBasis?:
        | "adapter_estimate"
        | "table_estimate"
        | "billed"
        | "estimated";
      /**
       * Per-turn token accounting, when the driver reported any. Additive and
       * every field optional; an unknown field is omitted, never sent as `0`.
       *
       * `inputTokens`, `cacheReadTokens` and `cacheWriteTokens` partition the
       * prompt side, so their sum is the turn's prompt-side total. That is
       * deliberately not the same convention as the item's own top-level
       * `inputTokens`, which is cache-*inclusive* and predates this block.
       *
       * The sum is turn *consumption*, not context occupancy: a turn that made
       * several model calls sent a prompt on each. A driver that states
       * occupancy directly does so in `context_window_updated`.
       */
      usage?: {
        inputTokens?: number;
        outputTokens?: number;
        cacheReadTokens?: number;
        cacheWriteTokens?: number;
        toolCalls?: number;
        contextWindow?: number;
      };
    }
  | { kind: "status"; status?: string }
  | {
      kind: "system_init";
      provider?: string;
      model?: string;
      tools?: string[];
      agents?: string[];
      slashCommands?: string[];
      mcpServers?: { name?: string; status?: string }[];
    }
  | { kind: "account_info"; accountInfo?: Record<string, unknown> }
  | { kind: "context_window_updated"; usage?: Record<string, unknown> }
  | { kind: "compact_boundary" }
  | { kind: "compact_summary"; summary?: string }
  | { kind: "context_cleared" }
  | { kind: "interrupted" }
  /**
   * The agent's own plan for the turn, as a replacement snapshot. `entries`
   * carries the structured per-step status; `text` is the same list rendered
   * as a markdown checklist so a reader that never learns the entry shape
   * still has something to show.
   */
  | {
      kind: "plan";
      entries?: {
        content?: string;
        priority?: string;
        status?: string;
      }[];
      text?: string;
    }
  /**
   * An item the producer could not fit inside the 32 KiB envelope cap. The
   * content is gone by construction — only its size and digest survive, so
   * the reader can say something was dropped and how much.
   */
  | {
      kind: "elided";
      reason?: string;
      byteCount?: number;
      contentDigest?: string;
    }
  /**
   * The agent's chain of reasoning. The donor forbade this outright; this fork
   * allows it deliberately (default-on via `BUZZ_CSP_INCLUDE_THOUGHTS`)
   * because reasoning is one of the things sessions are stored to analyse.
   */
  | { kind: "reasoning"; text?: unknown; provenance?: unknown }
);

/** Bounded quarantine record — safe metadata fields only, never payload. */
export interface CodingSessionQuarantineItemV1 {
  schema: "seat-transcript-quarantine/v1";
  quarantineClass?: string;
  decodeError?: string;
  sourceKey?: string;
  claimedKind?: string | null;
  claimedEntrySchema?: string | null;
  byteCount?: number;
  contentDigest?: string;
}
