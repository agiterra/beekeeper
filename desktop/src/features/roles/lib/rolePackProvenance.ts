/**
 * Provenance for one reported pack revision: *who was commissioned to run the
 * generation that reported it*, and when that cannot be answered, why not.
 *
 * A kind-44223 report says "this generation staged pack X". It carries no link
 * to the command that commissioned the generation — only the target tuple and
 * an echoed `sessionRef` — so a member of the channel can publish one for any
 * target they can name. This module joins the report back to signed evidence
 * and admits exactly one claim:
 *
 * > **Reported by a commissioned provider.** The 44223 is signed by the
 * > provider that an authorized, exact-generation lifecycle chain
 * > commissioned.
 *
 * It never claims the pack bytes ran. Staging is what the provider *said*; the
 * only thing verified here is the key that said it and the chain that entitled
 * it to speak.
 *
 * The rules that make this honest rather than comfortable:
 *
 * - **Exact generation only.** Proof is the accepted 44221 + 44224 pair for the
 *   row's *own* generation, taken from {@link foldSessionCoordination}. Nothing
 *   is inherited from a sibling or an earlier generation: a 44223 for
 *   generation 3 whose resume pair never landed reads `proof-unavailable`, even
 *   with generation 2 fully proven.
 * - **Founder means the signer of the genesis this create names, by exact event
 *   id.** No fallback to an umbrella projection, and none to execution
 *   metadata, both of which can be reached from provider-signed facts.
 * - **Recorded authority is historical.** Each command must be founder-signed
 *   or authorized by the receipt-backed operator timeline at its own timestamp.
 *   Current grants cannot retroactively authorize an older command. Missing
 *   history remains unavailable; contradictory bindings remain disputed.
 * - **An impostor row cannot overwrite a legitimate one.** Dispositions are
 *   keyed by {@link rolePackProvenanceKey} — channel, target, metadata event
 *   and signer — so two reports of one target from two signers keep two rows
 *   and two verdicts.
 *
 * Pure and deterministic: `now` comes from the caller, there is no module-level
 * state, and nothing here reads a clock or a cache.
 */

import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
import {
  classifyCodingSessionCreateEvent,
  classifyCodingSessionGenesisEvent,
} from "@/features/coding-sessions/lib/codingSessionCreateObservations";
import { encodeStructuredKey } from "@/features/coding-sessions/lib/codingSessionKeys";
import type { RelayEvent } from "@/shared/api/types";
import {
  coordinationByteOrder,
  foldSessionCoordination,
  type CoordinatedGeneration,
  type SessionCoordinationAmbiguity,
} from "@/shared/coordination/sessionCoordinationFold";
import { hasValidSignature } from "@/shared/lib/authors";
import {
  authorizeRoleOperatorCommand,
  buildRoleOperatorAuthorityEvidence,
} from "./roleOperatorCommissioning";

/** What this app is willing to say about one reported pack revision. */
export type RolePackProvenanceState =
  | "commissioned"
  | "proof-unavailable"
  | "disputed";

/** The verdict for one reported row, with the evidence it was reached from. */
export type RolePackProvenanceDisposition = {
  state: RolePackProvenanceState;
  /** Product sentence, null only for `commissioned`. */
  reason: string | null;
  /** Signer of the genesis this chain is bound to, once bound. */
  founderPubkey: string | null;
  /** Signer of this generation's own 44221, once it could be read. */
  commandSignerPubkey: string | null;
  commandEventId: string | null;
  receiptEventId: string | null;
  genesisEventId: string | null;
};

/** One reported pack revision, as the catalog holds it. */
export type RolePackProvenanceRow = {
  channelId: string;
  /** `buildCodingSessionTargetKey(session.commandTarget)`; null when absent. */
  targetKey: string | null;
  /** `session.statusEventId` — the 44223 this row was read from. */
  metadataEventId: string | null;
  /** `session.metadataAuthorityPubkey` — who signed that 44223. */
  signerPubkey: string | null;
  /** `session.sessionRef`, as the 44223 echoed it. */
  sessionRef: string | null;
};

/**
 * The disposition key: channel, target, metadata event, signer.
 *
 * The signer is part of the identity deliberately. Keying by target alone would
 * let a second 44223 for one target — the impostor case this module exists for
 * — replace the legitimate row's verdict instead of standing beside it.
 */
export function rolePackProvenanceKey(row: RolePackProvenanceRow): string {
  return [
    row.channelId ?? "",
    row.targetKey ?? "",
    row.metadataEventId ?? "",
    row.signerPubkey ?? "",
  ].join("|");
}

/**
 * One source read this client could not complete.
 *
 * `channelIds` names the chunk the failed or truncated read covered, so a
 * partial answer can be confined to the channels it actually affected. An
 * entry without it is a failure the reader cannot localise, and every channel
 * is treated as affected — under-claiming is the only safe direction.
 */
export type RolePackProvenanceSourceError = {
  scope: string;
  message: string;
  channelIds?: readonly string[];
};

/**
 * The read scopes that carry *proof*.
 *
 * A truncated or failed read of one of these can hide an older command,
 * receipt or genesis that contradicts what did arrive, so a row in an affected
 * channel may not be confirmed. A 44223 read is deliberately not on this list:
 * a missing report is a row that never renders, not a hidden contradiction.
 */
export const ROLE_PACK_PROVENANCE_PROOF_SCOPES = [
  "lifecycle-commands",
  "lifecycle-receipts",
  "session-genesis",
] as const;

/** Everything the model consumes. `now` is read once, after the last read. */
export type RolePackProvenanceInput = {
  /** Already signature-verified by the query module. */
  events: readonly RelayEvent[];
  channelIds: readonly string[];
  /** Unix seconds. */
  now: number;
  sourceErrors: readonly RolePackProvenanceSourceError[];
  rows: readonly RolePackProvenanceRow[];
  /** Trusted active relay identity; absence affects operator evidence only. */
  trustedRelayPubkey?: string | null;
};

/** The model's complete answer. */
export type RolePackProvenanceResult = {
  dispositions: ReadonlyMap<string, RolePackProvenanceDisposition>;
  /** Readable sentences: source-read failures and fold refusals. */
  notes: string[];
};

const NO_ACCEPTED_PROOF = "No accepted lifecycle proof for this generation.";
const NO_TARGET = "This report names no execution target.";
const CONTESTED_PROOF =
  "More than one lifecycle proof claims this generation, so none was accepted.";
const SIGNER_NOT_PROVIDER =
  "This report is signed by a key that is not the provider this generation was commissioned from.";
const COMMAND_UNREADABLE =
  "The lifecycle command that opened this generation could not be read.";
const NO_GENESIS_REF = "This generation's create names no genesis.";
const GENESIS_UNREADABLE = "The genesis this create cites could not be read.";
const GENESIS_OTHER_CHANNEL =
  "The genesis this create cites was published in a different channel than the create.";
const GENESIS_OTHER_SESSION =
  "The create and the genesis it cites name different sessions.";
const PROOF_OTHER_SESSION =
  "The accepted lifecycle proof names a different session than this create.";
const REPORT_NAMES_NO_SESSION = "This report names no session.";
const REPORT_OTHER_SESSION =
  "This report echoes a different session than the one this generation was commissioned for.";

const CHAIN_LOOP =
  "The resume chain for this generation refers back to itself and proves nothing.";
const INCOMPLETE_PROOF_READS =
  "Proof reads for this channel were incomplete, so this report cannot be confirmed:";
const UNRESOLVED_EVIDENCE =
  "Some lifecycle evidence in this project's channels could not be resolved, so it proves nothing.";

/** A short, quotable prefix — enough to find an event, never a dump. */
function shortId(value: string): string {
  return value.length > 8 ? `${value.slice(0, 8)}…` : value;
}

function endsAsSentence(value: string): string {
  const trimmed = value.trim();
  if (trimmed === "") return trimmed;
  return /[.!?…]$/.test(trimmed) ? trimmed : `${trimmed}.`;
}

function tagValue(event: RelayEvent, name: string): string | null {
  for (const tag of event.tags) {
    if (tag[0] === name && typeof tag[1] === "string") return tag[1];
  }
  return null;
}

/** `channel|targetKey`, length-prefixed so no field value can forge a match. */
function channelTargetKey(channelId: string, targetKey: string): string {
  return encodeStructuredKey("role-pack-provenance/v1", channelId, targetKey);
}

type TargetFields = {
  driver: string;
  instanceId: string;
  sessionId: string;
  generation: number;
};

/**
 * Decode a `coding-session/v1` target key back into its four fields.
 *
 * The row carries the key, not the tuple, and a refusal message names the
 * session id and generation. Decoding is total: the encoding writes a UTF-8
 * byte length, a colon, then exactly that many bytes, so a field containing a
 * colon or leading digits cannot be misread.
 */
export function decodeCodingSessionTargetKey(
  targetKey: string,
): TargetFields | null {
  const prefix = "coding-session/v1|";
  if (!targetKey.startsWith(prefix)) return null;
  const bytes = new TextEncoder().encode(targetKey.slice(prefix.length));
  const decoder = new TextDecoder("utf-8", { fatal: false });
  const fields: string[] = [];
  let offset = 0;
  for (let index = 0; index < 4; index += 1) {
    let digits = "";
    while (offset < bytes.length && bytes[offset] !== 0x3a) {
      digits += String.fromCharCode(bytes[offset]);
      offset += 1;
    }
    if (offset >= bytes.length || !/^\d{1,9}$/.test(digits)) return null;
    offset += 1;
    const length = Number.parseInt(digits, 10);
    if (offset + length > bytes.length) return null;
    fields.push(decoder.decode(bytes.subarray(offset, offset + length)));
    offset += length;
  }
  if (offset !== bytes.length) return null;
  const generation = Number.parseInt(fields[3], 10);
  if (!/^\d{1,9}$/.test(fields[3]) || generation <= 0) return null;
  return {
    driver: fields[0],
    instanceId: fields[1],
    sessionId: fields[2],
    generation,
  };
}

/** One accepted generation, joined back to the channel its command named. */
type GenerationProof = {
  channelId: string;
  targetKey: string;
  generation: CoordinatedGeneration;
  /** The fold session's `sessionRef` for this generation. */
  sessionRef: string | null;
  sessionId: string;
  generationNumber: number;
};

type ChainSigner = { generation: number; pubkey: string; event: RelayEvent };

type CreateFacts = {
  channelId: string;
  signerPubkey: string;
  sessionRef: string | null;
  genesisRef: string | null;
};

type ChainWalk =
  | {
      ok: true;
      /** Generation 1 first, this generation last. */
      signers: ChainSigner[];
      create: CreateFacts;
    }
  | { ok: false; state: RolePackProvenanceState; reason: string };

/**
 * The previous target of a resume or restart (which mints the next generation
 * the same way), read off the raw 44221.
 */
function readResumePreviousTarget(event: RelayEvent): unknown {
  let content: unknown;
  try {
    content = JSON.parse(event.content);
  } catch {
    return null;
  }
  if (typeof content !== "object" || content === null) return null;
  const action = (content as { action?: unknown }).action;
  if (typeof action !== "object" || action === null) return null;
  const typed = action as { type?: unknown; session?: unknown };
  if (typed.type !== "session.resume" && typed.type !== "session.restart") {
    return null;
  }
  return typed.session ?? null;
}

function targetKeyOf(session: unknown): string | null {
  if (typeof session !== "object" || session === null) return null;
  const target = session as Record<string, unknown>;
  if (
    typeof target.driver !== "string" ||
    typeof target.instanceId !== "string" ||
    typeof target.sessionId !== "string" ||
    !Number.isSafeInteger(target.generation) ||
    (target.generation as number) <= 0
  ) {
    return null;
  }
  return buildCodingSessionTargetKey({
    driver: target.driver,
    instanceId: target.instanceId,
    sessionId: target.sessionId,
    generation: target.generation as number,
  });
}

function ancestorMissing(generation: number): string {
  return `Generation ${generation} of this session has no accepted lifecycle proof, so the chain back to the first generation is incomplete.`;
}

/**
 * Walk the resume chain from this generation back to its generation-1 create,
 * collecting the signer of every command on the way.
 *
 * The fold already refuses to accept a resume whose predecessor is not itself
 * accepted, so a complete walk is the normal outcome; the incomplete branches
 * are kept because a broken chain must read as *unproven*, never as proven by
 * the generation below it.
 */
function walkChain(
  start: GenerationProof,
  proofs: ReadonlyMap<string, GenerationProof>,
  eventById: ReadonlyMap<string, RelayEvent>,
  channels: ReadonlySet<string>,
): ChainWalk {
  const signers: ChainSigner[] = [];
  const visited = new Set<string>();
  let current = start;
  for (;;) {
    const key = channelTargetKey(current.channelId, current.targetKey);
    if (visited.has(key)) {
      return { ok: false, state: "proof-unavailable", reason: CHAIN_LOOP };
    }
    visited.add(key);
    const event = eventById.get(current.generation.lifecycleCommandEventId);
    if (!event) {
      return {
        ok: false,
        state: "proof-unavailable",
        reason: COMMAND_UNREADABLE,
      };
    }
    const previousTarget = readResumePreviousTarget(event);
    if (previousTarget === null) {
      // Not a resume: this must be the generation-1 create, and the classifier
      // — signature, envelope and the exact action key set — decides whether
      // its authority facts may be read at all.
      const classified = classifyCodingSessionCreateEvent(event, channels);
      if (classified.kind !== "create") {
        return {
          ok: false,
          state: "proof-unavailable",
          reason: COMMAND_UNREADABLE,
        };
      }
      signers.push({
        generation: current.generationNumber,
        pubkey: classified.signerPubkey,
        event,
      });
      signers.reverse();
      return {
        ok: true,
        signers,
        create: {
          channelId: classified.channelId,
          signerPubkey: classified.signerPubkey,
          sessionRef: classified.sessionRef,
          genesisRef: classified.genesisRef,
        },
      };
    }
    // The fold verifies no signatures; a resume has no classifier of its own,
    // so the signature is checked here before its signer joins the chain.
    if (!hasValidSignature(event)) {
      return {
        ok: false,
        state: "proof-unavailable",
        reason: COMMAND_UNREADABLE,
      };
    }
    signers.push({
      generation: current.generationNumber,
      pubkey: event.pubkey,
      event,
    });
    const previousKey = targetKeyOf(previousTarget);
    const previous = previousKey
      ? proofs.get(channelTargetKey(current.channelId, previousKey))
      : undefined;
    if (!previous) {
      return {
        ok: false,
        state: "proof-unavailable",
        reason: ancestorMissing(current.generationNumber - 1),
      };
    }
    current = previous;
  }
}

const AMBIGUITY_CONFLICT =
  /^command (.+) has \d+ commands and \d+ receipts; no generation was accepted$/;
const AMBIGUITY_SIGNER =
  /^command (.+) was signed by [0-9a-f]{64}, which may not commission an execution of this session and answers no accepted hire; no generation was accepted$/;
const AMBIGUITY_PROOFS =
  /^target (.+) generation (\d+) is claimed by \d+ distinct lifecycle proofs; none was accepted$/;
const AMBIGUITY_LEASES =
  /^target (.+) generation (\d+) has \d+ distinct leases at sequence \d+; none proves reachability$/;

/**
 * Turn one fold refusal into a sentence a person can act on.
 *
 * The fold's own messages are diagnostics carrying full pubkeys and command
 * ids; printed on the Packs tab they read as noise and teach nobody anything.
 * An unrecognised message becomes the honest generic sentence rather than a
 * raw dump.
 */
function describeAmbiguity(ambiguity: SessionCoordinationAmbiguity): string {
  const conflict = AMBIGUITY_CONFLICT.exec(ambiguity.message);
  if (conflict) {
    return `A lifecycle command (${shortId(conflict[1])}) is answered by conflicting evidence, so no generation was proven from it.`;
  }
  const signer = AMBIGUITY_SIGNER.exec(ambiguity.message);
  if (signer) {
    return `A lifecycle command (${shortId(signer[1])}) was signed by a key that may not commission this session, so no generation was proven from it.`;
  }
  const proofs = AMBIGUITY_PROOFS.exec(ambiguity.message);
  if (proofs) {
    return `Generation ${proofs[2]} of session ${shortId(proofs[1])} is claimed by more than one lifecycle proof, so none was accepted.`;
  }
  const leases = AMBIGUITY_LEASES.exec(ambiguity.message);
  if (leases) {
    return `Generation ${leases[2]} of session ${shortId(leases[1])} has more than one lease at the same sequence, so none proves reachability.`;
  }
  return UNRESOLVED_EVIDENCE;
}

/** True when a fold refusal names this exact target's session and generation. */
function ambiguityNamesTarget(
  ambiguities: readonly SessionCoordinationAmbiguity[],
  fields: TargetFields,
): boolean {
  const prefix = `target ${fields.sessionId} generation ${fields.generation} `;
  return ambiguities.some(
    (ambiguity) =>
      ambiguity.scope === "authority" && ambiguity.message.startsWith(prefix),
  );
}

/** Which channels lost part of a proof read, and the sentence that says so. */
type IncompleteProofReads = {
  /** Failures the reader could not localise; they affect every channel. */
  everywhere: string[];
  byChannel: Map<string, string[]>;
};

/**
 * Group the proof-kind read failures by the channels they actually cover.
 *
 * Only the three proof scopes count. A failed 44223 read costs the reader
 * rows, not proof — a report that never arrived renders nothing to confirm —
 * and an event dropped for an invalid signature is evidence the reader was
 * right to refuse, not evidence it is missing something.
 */
function incompleteProofReads(
  sourceErrors: readonly RolePackProvenanceSourceError[],
): IncompleteProofReads {
  const proofScopes = new Set<string>(ROLE_PACK_PROVENANCE_PROOF_SCOPES);
  const everywhere: string[] = [];
  const byChannel = new Map<string, string[]>();
  for (const error of sourceErrors) {
    if (!proofScopes.has(error.scope)) continue;
    const sentence = endsAsSentence(error.message);
    if (sentence === "") continue;
    if (!error.channelIds) {
      everywhere.push(sentence);
      continue;
    }
    for (const channelId of error.channelIds) {
      byChannel.set(channelId, [...(byChannel.get(channelId) ?? []), sentence]);
    }
  }
  return { everywhere, byChannel };
}

/** The incomplete-read sentences that apply to one channel, deduped and ordered. */
function incompleteSentencesFor(
  incomplete: IncompleteProofReads,
  channelId: string,
): string[] {
  return [
    ...new Set([
      ...incomplete.everywhere,
      ...(incomplete.byChannel.get(channelId) ?? []),
    ]),
  ].sort(coordinationByteOrder);
}

function unavailable(
  reason: string,
  evidence: Partial<RolePackProvenanceDisposition> = {},
): RolePackProvenanceDisposition {
  return {
    state: "proof-unavailable",
    reason,
    founderPubkey: null,
    commandSignerPubkey: null,
    commandEventId: null,
    receiptEventId: null,
    genesisEventId: null,
    ...evidence,
  };
}

function disputed(
  reason: string,
  evidence: Partial<RolePackProvenanceDisposition> = {},
): RolePackProvenanceDisposition {
  return { ...unavailable(reason, evidence), state: "disputed", reason };
}

/**
 * Decide the provenance of every reported pack-revision row.
 *
 * The order of the checks is the order of the claims they defend, weakest
 * evidence first: without an accepted generation nothing else can be asked;
 * with one, the signer of the report must be that generation's provider before
 * any question of authority arises; only then is the chain walked, and only a
 * complete, authorized chain earns the commissioned label.
 */
export function buildRolePackProvenance(
  input: RolePackProvenanceInput,
): RolePackProvenanceResult {
  const channels = new Set(input.channelIds);
  const eventById = new Map<string, RelayEvent>();
  for (const event of input.events) {
    if (!eventById.has(event.id)) eventById.set(event.id, event);
  }

  const foldInput = {
    now: input.now,
    // The fold's shape is the signed seven fields minus `sig`; the classifiers
    // keep the signed events and verify them again for themselves.
    events: input.events.map((event) => ({
      id: event.id,
      pubkey: event.pubkey,
      created_at: event.created_at,
      kind: event.kind,
      tags: event.tags,
      content: event.content,
    })),
    // The fold's vocabulary is `{scope, message}`; the channel scoping this
    // module adds is its own business and stays out of the shared type.
    sourceErrors: input.sourceErrors.map((error) => ({
      scope: error.scope,
      message: error.message,
    })),
    commissioners: undefined,
  };
  const authorityCache = new Map<
    string,
    ReturnType<typeof buildRoleOperatorAuthorityEvidence>
  >();
  const authorize: DispositionContext["authorize"] = (genesis, command) => {
    if (command.pubkey === genesis.founderPubkey)
      return { authorized: true, reason: null };
    const key = channelTargetKey(genesis.channelId, genesis.eventId);
    let authority = authorityCache.get(key);
    if (!authority) {
      authority = buildRoleOperatorAuthorityEvidence({
        events: input.events,
        trustedRelayPubkey: input.trustedRelayPubkey ?? null,
        scope: {
          channelId: genesis.channelId,
          genesisRef: genesis.eventId,
          founderPubkey: genesis.founderPubkey,
        },
        sourceComplete: !input.sourceErrors.some(
          (error) =>
            ["authority-transitions", "authority-receipts"].includes(
              error.scope,
            ) &&
            (!error.channelIds || error.channelIds.includes(genesis.channelId)),
        ),
      });
      authorityCache.set(key, authority);
    }
    return authorizeRoleOperatorCommand(authority, command);
  };
  // Creates name their exact genesis, so authorize them independently of
  // target uniqueness. In particular, a competing authorized self-provider
  // create must remain present when the final fold detects contradictions.
  const commissionedCommandEventIds = new Set<string>();
  const selfResumes = new Set<string>();
  for (const event of input.events) {
    if (event.kind !== 44221) continue;
    const create = classifyCodingSessionCreateEvent(event, channels);
    if (create.kind === "create" && create.genesisRef) {
      const genesisEvent = eventById.get(create.genesisRef);
      const genesis = genesisEvent
        ? classifyCodingSessionGenesisEvent(genesisEvent, channels)
        : null;
      if (
        genesis?.kind === "genesis" &&
        genesis.channelId === create.channelId &&
        genesis.sessionRef === create.sessionRef &&
        authorize(genesis, event).authorized
      ) {
        commissionedCommandEventIds.add(event.id);
      }
    } else if (readResumePreviousTarget(event) !== null) {
      const action = JSON.parse(event.content).action;
      if (action.providerAuthorityPubkey === event.pubkey)
        selfResumes.add(event.id);
    }
  }
  let provisionalAmbiguities: readonly SessionCoordinationAmbiguity[] = [];
  // Ordinary founder/operator commands need just the ordinary fold. Only a
  // resume whose operator is also its provider needs private lineage discovery.
  // This provisional answer is never exposed as confirmation.
  if (selfResumes.size > 0) {
    const candidates = foldSessionCoordination({
      ...foldInput,
      commissionedCommandEventIds: new Set([
        ...commissionedCommandEventIds,
        ...selfResumes,
      ]),
    });
    const candidateProofs = collectProofs(candidates.sessions, eventById);
    provisionalAmbiguities = candidates.ambiguities;
    const candidateContext: DispositionContext = {
      proofs: candidateProofs,
      eventById,
      channels,
      authorize,
      ambiguities: candidates.ambiguities,
      provisionalAmbiguities: [],
      noProofReason: NO_ACCEPTED_PROOF,
    };
    // Validate deepest lineages first; their verified command IDs cover earlier
    // generations without walking those same ancestors again.
    for (const proof of [...candidateProofs.values()].sort(
      (a, b) => b.generationNumber - a.generationNumber,
    )) {
      const id = proof.generation.lifecycleCommandEventId;
      if (!selfResumes.has(id) || commissionedCommandEventIds.has(id)) continue;
      const result = disposeRow(
        {
          channelId: proof.channelId,
          targetKey: proof.targetKey,
          signerPubkey: proof.generation.providerAuthorityPubkey,
          sessionRef: proof.sessionRef,
          metadataEventId: null,
        },
        candidateContext,
      );
      if (result.state !== "commissioned") continue;
      const chain = walkChain(proof, candidateProofs, eventById, channels);
      if (chain.ok)
        for (const signer of chain.signers)
          commissionedCommandEventIds.add(signer.event.id);
    }
  }
  const fold = foldSessionCoordination({
    ...foldInput,
    commissionedCommandEventIds,
  });
  const proofs = collectProofs(fold.sessions, eventById);

  const sourceSentences = [
    ...new Set(fold.errors.map((error) => endsAsSentence(error.message))),
  ].filter((sentence) => sentence !== "");
  const noProofReason =
    sourceSentences.length === 0
      ? NO_ACCEPTED_PROOF
      : `${NO_ACCEPTED_PROOF} ${sourceSentences.join(" ")}`;

  const incomplete = incompleteProofReads(input.sourceErrors);
  const context: DispositionContext = {
    proofs,
    eventById,
    channels,
    ambiguities: fold.ambiguities,
    noProofReason,
    provisionalAmbiguities,
    authorize,
  };
  const dispositions = new Map<string, RolePackProvenanceDisposition>();
  for (const row of input.rows) {
    const disposition = disposeRow(row, context);
    const unread = incompleteSentencesFor(incomplete, row.channelId);
    dispositions.set(
      rolePackProvenanceKey(row),
      // A positive earned from a partial read is the one verdict this module
      // may not give: the page that did not arrive is exactly where an older
      // conflicting command would be. A contradiction already found stands —
      // a missing page cannot un-contradict evidence that is in hand.
      disposition.state === "commissioned" && unread.length > 0
        ? {
            ...disposition,
            state: "proof-unavailable",
            reason: `${INCOMPLETE_PROOF_READS} ${unread.join(" ")}`,
          }
        : disposition,
    );
  }

  const notes = [
    ...new Set([
      ...sourceSentences,
      ...fold.ambiguities.map(describeAmbiguity),
    ]),
  ].sort(coordinationByteOrder);

  return { dispositions, notes };
}

function collectProofs(
  sessions: ReturnType<typeof foldSessionCoordination>["sessions"],
  eventById: ReadonlyMap<string, RelayEvent>,
): Map<string, GenerationProof> {
  const proofs = new Map<string, GenerationProof>();
  for (const session of sessions) {
    for (const generation of session.generations) {
      const command = eventById.get(generation.lifecycleCommandEventId);
      const channelId = command ? tagValue(command, "h") : null;
      const fields = decodeCodingSessionTargetKey(generation.targetKey);
      if (!channelId || !fields) continue;
      proofs.set(channelTargetKey(channelId, generation.targetKey), {
        channelId,
        targetKey: generation.targetKey,
        generation,
        sessionRef: session.sessionRef,
        sessionId: fields.sessionId,
        generationNumber: fields.generation,
      });
    }
  }

  return proofs;
}

type DispositionContext = {
  proofs: ReadonlyMap<string, GenerationProof>;
  eventById: ReadonlyMap<string, RelayEvent>;
  channels: ReadonlySet<string>;
  ambiguities: readonly SessionCoordinationAmbiguity[];
  noProofReason: string;
  provisionalAmbiguities: readonly SessionCoordinationAmbiguity[];
  authorize: (
    genesis: { eventId: string; channelId: string; founderPubkey: string },
    command: RelayEvent,
  ) => { authorized: boolean; reason: string | null };
};

function disposeRow(
  row: RolePackProvenanceRow,
  context: DispositionContext,
): RolePackProvenanceDisposition {
  if (!row.targetKey) return unavailable(NO_TARGET);
  const fields = decodeCodingSessionTargetKey(row.targetKey);
  const proof = context.proofs.get(
    channelTargetKey(row.channelId, row.targetKey),
  );
  if (!proof) {
    // Nothing accepted this generation. That is contradiction only when the
    // fold refused a proof that named this very target; otherwise the evidence
    // is simply not here — not loaded, not published, or never minted.
    if (fields && ambiguityNamesTarget(context.ambiguities, fields)) {
      return disputed(CONTESTED_PROOF);
    }
    return unavailable(context.noProofReason);
  }

  if (fields && ambiguityNamesTarget(context.provisionalAmbiguities, fields)) {
    return unavailable(
      "Competing lifecycle claims could not all be resolved against commissioning authority.",
    );
  }
  const commandEvent = context.eventById.get(
    proof.generation.lifecycleCommandEventId,
  );
  const evidence = {
    commandSignerPubkey: commandEvent?.pubkey ?? null,
    commandEventId: proof.generation.lifecycleCommandEventId,
    receiptEventId: proof.generation.lifecycleReceiptEventId,
  };

  if (row.signerPubkey !== proof.generation.providerAuthorityPubkey) {
    return disputed(SIGNER_NOT_PROVIDER, evidence);
  }

  const chain = walkChain(
    proof,
    context.proofs,
    context.eventById,
    context.channels,
  );
  if (!chain.ok) {
    return chain.state === "disputed"
      ? disputed(chain.reason, evidence)
      : unavailable(chain.reason, evidence);
  }

  const genesisRef = chain.create.genesisRef;
  if (!genesisRef) return unavailable(NO_GENESIS_REF, evidence);
  const genesisEvent = context.eventById.get(genesisRef);
  if (!genesisEvent) return unavailable(GENESIS_UNREADABLE, evidence);
  const genesis = classifyCodingSessionGenesisEvent(
    genesisEvent,
    context.channels,
  );
  if (genesis.kind !== "genesis") {
    return unavailable(GENESIS_UNREADABLE, evidence);
  }
  const bound = { ...evidence, genesisEventId: genesis.eventId };

  if (genesis.channelId !== chain.create.channelId) {
    return disputed(GENESIS_OTHER_CHANNEL, bound);
  }
  if (genesis.sessionRef !== chain.create.sessionRef) {
    return disputed(GENESIS_OTHER_SESSION, bound);
  }
  if (proof.sessionRef !== chain.create.sessionRef) {
    return disputed(PROOF_OTHER_SESSION, bound);
  }
  const founded = { ...bound, founderPubkey: genesis.founderPubkey };
  if (row.sessionRef === null) {
    return unavailable(REPORT_NAMES_NO_SESSION, founded);
  }
  if (row.sessionRef !== genesis.sessionRef) {
    return disputed(REPORT_OTHER_SESSION, founded);
  }

  // Every command on the chain, generation 1 first: the earliest key that is
  // not the founder is the one that broke the chain, and naming a later one
  // would send a reader looking in the wrong place.
  for (const signer of chain.signers) {
    const previous = targetKeyOf(readResumePreviousTarget(signer.event));
    const previousFields = previous
      ? decodeCodingSessionTargetKey(previous)
      : null;
    if (
      previousFields &&
      ambiguityNamesTarget(context.provisionalAmbiguities, previousFields)
    ) {
      return unavailable(
        "An ancestor has competing lifecycle claims whose commissioning authority could not be resolved.",
        founded,
      );
    }
    const authority = context.authorize(genesis, signer.event);
    if (!authority.authorized) {
      return unavailable(
        `Generation ${signer.generation}: ${authority.reason ?? "The command signer has no confirmed commissioning authority."}`,
        founded,
      );
    }
  }

  return { ...founded, state: "commissioned", reason: null };
}
