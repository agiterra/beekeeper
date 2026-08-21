import { buildCodingSessionTranscriptGenerationId } from "./codingSessionTranscriptPresentation";
import {
  type CodingSessionLifecycleResolution,
  establishedCodingSessionTarget,
} from "./codingSessionTrustedIngress";
import { codingSessionResumedWithoutContextMessage } from "./newCodingSessionModel";

/**
 * What a published `session.resume` settled into.
 *
 * A resume is not finished when the 44221 is accepted — the provider answers
 * it by minting the NEXT generation and publishing a receipt naming that new
 * target. The routed generation the person pressed Reconnect from is frozen
 * `disconnected` forever, so the only honest outcomes are: still waiting, an
 * established new generation to follow, or a refusal to show.
 */
export type CodingSessionResumeSettlement =
  | { kind: "pending" }
  | {
      kind: "established";
      commandId: string;
      /**
       * The catalog generation id of the generation the provider resumed
       * into, built exactly as {@link useCodingSessionCatalog} builds it.
       */
      generationId: string;
      /** Context-loss copy when the provider recovered no prior context. */
      notice: string | null;
    }
  | { kind: "failed"; commandId: string; message: string };

/** Fallback when a refusal receipt carries no readable message. */
export const CODING_SESSION_RESUME_REFUSED_MESSAGE =
  "The provider refused this reconnect.";

/** A receipt signed by an authority this command never addressed. */
export const CODING_SESSION_RESUME_CONFLICT_MESSAGE =
  "Conflicting reconnect receipts were signed for this command; nothing was resumed.";

/**
 * Silence is not success. The lifecycle resolution is clock-free by design, so
 * the deadline lives in the caller — thirty seconds without a receipt means a
 * step failed somewhere signed facts cannot reach, and the person needs the
 * Reconnect button back rather than a spinner that never ends.
 */
export const CODING_SESSION_RESUME_STALL_MESSAGE =
  "The provider has not answered this reconnect. It may no longer be running — try again, or stop the execution.";

/**
 * Read one resume command's signed lifecycle resolution.
 *
 * Pure on purpose: every branch is a reading of provider-signed facts, so the
 * hook around it only has to decide where to navigate and what to say.
 */
export function resolveCodingSessionResumeSettlement(input: {
  channelId: string;
  lifecycle: CodingSessionLifecycleResolution | null | undefined;
  providerAuthorityPubkey: string | null;
}): CodingSessionResumeSettlement {
  const lifecycle = input.lifecycle;
  if (!lifecycle || !input.providerAuthorityPubkey) return { kind: "pending" };
  if (lifecycle.state === "failed") {
    // The provider refuses a stale generation with a signed receipt
    // (`STALE_GENERATION`). Without this branch that refusal is published,
    // verified, and then dropped on the floor — the silence the person reads
    // as a broken button.
    return {
      kind: "failed",
      commandId: lifecycle.commandId,
      message:
        lifecycle.error.message.trim() || CODING_SESSION_RESUME_REFUSED_MESSAGE,
    };
  }
  if (lifecycle.state === "conflict") {
    return {
      kind: "failed",
      commandId: lifecycle.commandId,
      message: CODING_SESSION_RESUME_CONFLICT_MESSAGE,
    };
  }
  const target = establishedCodingSessionTarget(lifecycle);
  if (!target) return { kind: "pending" };
  return {
    kind: "established",
    commandId: lifecycle.commandId,
    generationId: buildCodingSessionTranscriptGenerationId(
      input.channelId,
      input.providerAuthorityPubkey,
      target,
    ),
    // A resume that recovered nothing still opens: the session and its durable
    // transcript are intact. The loss is told, never used to withhold the
    // generation the person asked to get back to.
    notice:
      lifecycle.state === "resumed-without-context"
        ? codingSessionResumedWithoutContextMessage(lifecycle.error)
        : null,
  };
}
