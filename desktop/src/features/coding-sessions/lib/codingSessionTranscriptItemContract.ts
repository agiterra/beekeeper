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
      };
    }
  | {
      kind: "tool_result";
      toolId?: string;
      toolName?: string;
      /** The opening call's ACP discriminant, carried onto its result. */
      toolKind?: string;
      content?: unknown;
      isError?: boolean;
    }
  | {
      kind: "result";
      subtype?: "success" | "error" | "cancelled";
      isError?: boolean;
      durationMs?: number;
      result?: string;
      costUsd?: number;
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
