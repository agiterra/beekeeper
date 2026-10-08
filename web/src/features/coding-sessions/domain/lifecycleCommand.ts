/**
 * The 44221 lifecycle command, as the observer reads it.
 *
 * This is the trust source: a create names the `providerAuthorityPubkey`
 * whose 44223/44224/44225 may be believed for the executions it mints (D5),
 * and the `genesisRef` that anchors the umbrella's founder (D7). Mirrors the
 * decode half of
 * `desktop/src/features/coding-sessions/lib/codingSessionLifecycleCommand.ts`
 * and the strict tag discipline of desktop's `sessionCoordinationFold`.
 */
import { KIND_CODING_SESSION_LIFECYCLE_COMMAND } from "../../../shared/lib/kinds.ts";
import type { CodingSessionTarget, ObservedEvent } from "./types.ts";
import {
  boundedNonempty,
  decodeTarget,
  hasExactKeys,
  hasOwnKey,
  isStrictRoutingRecord,
  hasRequiredAndOptionalKeys,
  isCodingSessionSessionRef,
  isExactProviderAuthorityPubkey,
  isPlainRecord,
  normalizePubkey,
  parseBoundedJson,
  parseExactTags,
} from "./wireDecode.ts";

export const CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA =
  "buzz-coding-session-lifecycle-command/v1" as const;
export const CODING_SESSION_LIFECYCLE_COMMAND_TAG_VERSION = "csl1-1" as const;

const MAX_LIFECYCLE_CONTENT_BYTES = 16 * 1024;
const MAX_IDENTIFIER_BYTES = 256;
const MAX_REFERENCE_BYTES = 2 * 1024;

/** A decoded, signature-checked 44221. */
export type CodingSessionLifecycleCommand = {
  eventId: string;
  channelId: string;
  createdAt: number;
  /** The human who signed it. */
  signerPubkey: string;
  commandId: string;
  action: "create" | "resume" | "restart" | "rewind" | "stop";
  /** The provider this command names — the only signer its answers may carry. */
  providerAuthorityPubkey: string;
  /** Create only. */
  sessionRef: string | null;
  genesisRef: string | null;
  projectRef: string | null;
  repoRef: string | null;
  model: string | null;
  title: string | null;
  /** Resume/restart/rewind/stop only: the target the command addresses. */
  previousTarget: CodingSessionTarget | null;
  /**
   * Rewind only (SV-29): the 44231 turn checkpoint it cuts at, and whether
   * the working tree is kept or restored. A rewind mints the next
   * generation exactly as a resume or restart does.
   */
  rewind?: { checkpoint: string; files: "keep" | "restore" };
};

const HEX64 = /^[0-9a-f]{64}$/;

/**
 * Decode one 44221, or null.
 *
 * The caller verifies the signature before handing the event in — this module
 * stays pure so it can be unit-tested without a crypto dependency.
 */
export function parseCodingSessionLifecycleCommand(
  event: ObservedEvent,
): CodingSessionLifecycleCommand | null {
  if (event.kind !== KIND_CODING_SESSION_LIFECYCLE_COMMAND) return null;
  const value = parseBoundedJson(event.content, MAX_LIFECYCLE_CONTENT_BYTES);
  if (
    !isPlainRecord(value) ||
    !hasExactKeys(value, ["schema", "commandId", "action"]) ||
    value.schema !== CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA ||
    !boundedNonempty(value.commandId, MAX_IDENTIFIER_BYTES) ||
    !isPlainRecord(value.action)
  ) {
    return null;
  }
  const tags = parseExactTags(event.tags, ["h", "csl-v", "csl-command"]);
  if (
    !tags ||
    tags[0].length === 0 ||
    tags[1] !== CODING_SESSION_LIFECYCLE_COMMAND_TAG_VERSION ||
    tags[2] !== value.commandId
  ) {
    return null;
  }
  const signerPubkey = normalizePubkey(event.pubkey);
  if (!signerPubkey) return null;
  const action = value.action;
  if (!isExactProviderAuthorityPubkey(action.providerAuthorityPubkey)) {
    return null;
  }
  const base = {
    eventId: event.id,
    channelId: tags[0],
    createdAt: event.created_at,
    signerPubkey,
    commandId: value.commandId,
    providerAuthorityPubkey: action.providerAuthorityPubkey,
  };
  if (action.type === "session.create") {
    return decodeCreate(base, action);
  }
  if (action.type === "session.rewind") {
    const previousTarget = decodeTarget(action.session);
    if (
      !previousTarget ||
      !hasExactKeys(action, [
        "type",
        "session",
        "providerAuthorityPubkey",
        "checkpoint",
        "files",
      ]) ||
      typeof action.checkpoint !== "string" ||
      !HEX64.test(action.checkpoint) ||
      (action.files !== "keep" && action.files !== "restore")
    ) {
      return null;
    }
    return {
      ...base,
      action: "rewind",
      sessionRef: null,
      genesisRef: null,
      projectRef: null,
      repoRef: null,
      model: null,
      title: null,
      previousTarget,
      rewind: { checkpoint: action.checkpoint, files: action.files },
    };
  }
  if (
    action.type === "session.resume" ||
    action.type === "session.restart" ||
    action.type === "session.stop"
  ) {
    const previousTarget = decodeTarget(action.session);
    if (
      !previousTarget ||
      !hasExactKeys(action, ["type", "session", "providerAuthorityPubkey"])
    ) {
      return null;
    }
    return {
      ...base,
      action:
        action.type === "session.resume"
          ? "resume"
          : action.type === "session.restart"
            ? "restart"
            : "stop",
      sessionRef: null,
      genesisRef: null,
      projectRef: null,
      repoRef: null,
      model: null,
      title: null,
      previousTarget,
    };
  }
  return null;
}

function decodeCreate(
  base: Omit<
    CodingSessionLifecycleCommand,
    | "action"
    | "sessionRef"
    | "genesisRef"
    | "projectRef"
    | "repoRef"
    | "model"
    | "title"
    | "previousTarget"
  >,
  action: Record<string, unknown>,
): CodingSessionLifecycleCommand | null {
  const required = [
    "type",
    "projectRef",
    "repoRef",
    "providerInstanceRef",
    "providerAuthorityPubkey",
    "model",
    "title",
    "initialTurn",
  ] as const;
  // An agent seat: `actor` and `role` together or not at all, in the exact
  // forms buzz-core accepts (lowercase 64-hex; `[a-z0-9-]` slug).
  const hasActor = hasOwnKey(action, "actor");
  if (hasActor !== hasOwnKey(action, "role")) return null;
  if (
    hasActor &&
    (typeof action.actor !== "string" ||
      !/^[0-9a-f]{64}$/.test(action.actor) ||
      typeof action.role !== "string" ||
      !/^[a-z0-9-]{1,64}$/.test(action.role))
  ) {
    return null;
  }
  if (
    !hasRequiredAndOptionalKeys(action, required, [
      "sessionRef",
      "genesisRef",
      "actor",
      "role",
      // The 2026-08-30 routing amendment. Optional and trailing: an unrouted
      // create is the same shape it always was, and a routed one is read
      // rather than dropped — a strict list that forgets an amendment does
      // not render a nuance, it renders an empty session list.
      "routing",
    ]) ||
    (hasOwnKey(action, "routing") && !isStrictRoutingRecord(action.routing)) ||
    !nullableBounded(action.projectRef, MAX_REFERENCE_BYTES) ||
    !nullableBounded(action.repoRef, MAX_REFERENCE_BYTES) ||
    !nullableBounded(action.model, MAX_REFERENCE_BYTES) ||
    !nullableBounded(action.title, MAX_REFERENCE_BYTES) ||
    !boundedNonempty(action.providerInstanceRef, MAX_IDENTIFIER_BYTES)
  ) {
    return null;
  }
  // `sessionRef` is optional (pre-umbrella creates omit the key) but never
  // loose: when present it is either an explicit null or the canonical UUID.
  let sessionRef: string | null = null;
  if (hasOwnKey(action, "sessionRef")) {
    if (action.sessionRef === null) {
      sessionRef = null;
    } else if (isCodingSessionSessionRef(action.sessionRef)) {
      sessionRef = action.sessionRef;
    } else {
      return null;
    }
  }
  // A genesis anchor without a session to anchor is malformed, not legacy.
  let genesisRef: string | null = null;
  if (hasOwnKey(action, "genesisRef")) {
    if (
      typeof action.genesisRef !== "string" ||
      !/^[0-9a-f]{64}$/.test(action.genesisRef) ||
      sessionRef === null
    ) {
      return null;
    }
    genesisRef = action.genesisRef;
  }
  return {
    ...base,
    action: "create",
    sessionRef,
    genesisRef,
    projectRef: action.projectRef as string | null,
    repoRef: action.repoRef as string | null,
    model: action.model as string | null,
    title: action.title as string | null,
    previousTarget: null,
  };
}

function nullableBounded(value: unknown, maxBytes: number): boolean {
  return value === null || boundedNonempty(value, maxBytes);
}
