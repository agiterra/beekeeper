import assert from "node:assert/strict";

import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand.ts";
import { buildCodingSessionGenesisEvent } from "@/features/coding-sessions/lib/codingSessionGenesis.ts";
import {
  buildCodingSessionCreateEvent,
  buildCodingSessionResumeEvent,
} from "@/features/coding-sessions/lib/codingSessionLifecycleCommand.ts";
import {
  CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION,
  lifecycleReceiptSemanticKey,
} from "@/features/coding-sessions/lib/codingSessionTrustedIngress.ts";
import {
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_METADATA,
} from "@/shared/constants/kinds.ts";

import {
  buildRolePackProvenance,
  rolePackProvenanceKey,
} from "./rolePackProvenance.ts";

const CHANNEL = "channel-provenance-1";
const OTHER_CHANNEL = "channel-provenance-2";
const CHANNELS = [CHANNEL, OTHER_CHANNEL];
const NOW = 1_800_100_000;

const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const OTHER_SESSION_REF = "7c9f3d1b-22a4-4c6e-9d0f-1a2b3c4d5e6f";
const THIRD_SESSION_REF = "9e1a2b3c-44d5-4e7f-8a9b-0c1d2e3f4a5b";

const FOUNDER_SECRET = generateSecretKey();
const FOUNDER = getPublicKey(FOUNDER_SECRET);
const PROVIDER_SECRET = generateSecretKey();
const PROVIDER = getPublicKey(PROVIDER_SECRET);
const IMPOSTOR_X_SECRET = generateSecretKey();
const IMPOSTOR_X = getPublicKey(IMPOSTOR_X_SECRET);
const IMPOSTOR_Y_SECRET = generateSecretKey();
const IMPOSTOR_Y = getPublicKey(IMPOSTOR_Y_SECRET);
const OPERATOR_SECRET = generateSecretKey();

/** One execution's targets. Generation is part of the identity, always. */
function target(sessionId, generation) {
  return {
    driver: "claude-agent-acp",
    instanceId: "claude-instance",
    sessionId,
    generation,
  };
}

const SESSION_ID = "11111111-1111-1111-1111-111111111111";
const IMPOSTOR_SESSION_ID = "22222222-2222-2222-2222-222222222222";

const T1 = target(SESSION_ID, 1);
const T2 = target(SESSION_ID, 2);
const T3 = target(SESSION_ID, 3);

function createEvent({
  secret = FOUNDER_SECRET,
  commandId,
  channelId = CHANNEL,
  sessionRef = SESSION_REF,
  genesisRef,
  hireRef,
  provider = PROVIDER,
  createdAt = 1_800_000_000,
}) {
  const built = buildCodingSessionCreateEvent({
    channelId,
    commandId,
    projectRef: null,
    repoRef: null,
    sessionRef,
    ...(genesisRef ? { genesisRef } : {}),
    ...(hireRef ? { hireRef } : {}),
    providerInstanceRef: "claude-primary",
    providerAuthorityPubkey: provider,
    model: null,
    title: "Advance the Packs tab",
    initialTurn: null,
  });
  return finalizeEvent(
    {
      kind: built.kind,
      created_at: createdAt,
      tags: built.tags,
      content: built.content,
    },
    secret,
  );
}

function resumeEvent({
  secret = FOUNDER_SECRET,
  commandId,
  channelId = CHANNEL,
  previous,
  provider = PROVIDER,
  createdAt = 1_800_000_100,
}) {
  const built = buildCodingSessionResumeEvent({
    channelId,
    commandId,
    target: previous,
    providerAuthorityPubkey: provider,
  });
  return finalizeEvent(
    {
      kind: built.kind,
      created_at: createdAt,
      tags: built.tags,
      content: built.content,
    },
    secret,
  );
}

function receiptEvent({
  secret = PROVIDER_SECRET,
  commandId,
  channelId = CHANNEL,
  minted,
  status = "created",
  createdAt = 1_800_000_005,
}) {
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
      created_at: createdAt,
      tags: [
        ["h", channelId],
        ["cslr-v", CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION],
        ["csl-command", commandId],
        ["csl-key", lifecycleReceiptSemanticKey(commandId)],
      ],
      content: JSON.stringify({
        schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
        commandId,
        status,
        session: minted,
        error: null,
      }),
    },
    secret,
  );
}

function genesisEvent({
  secret = FOUNDER_SECRET,
  channelId = CHANNEL,
  sessionRef = SESSION_REF,
  createdAt = 1_799_999_999,
}) {
  const built = buildCodingSessionGenesisEvent({ channelId, sessionRef });
  return finalizeEvent(
    {
      kind: built.kind,
      created_at: createdAt,
      tags: built.tags,
      content: built.content,
    },
    secret,
  );
}

/** A real 44223 in the strict shape the shared decoder accepts. */
function metadataEvent({
  secret = PROVIDER_SECRET,
  channelId = CHANNEL,
  reported,
  sessionRef = SESSION_REF,
  status = "running",
  createdAt = 1_800_000_010,
}) {
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_METADATA,
      created_at: createdAt,
      tags: [
        ["h", channelId],
        ["cs-target", buildCodingSessionTargetKey(reported)],
      ],
      content: JSON.stringify({
        schema: "buzz-coding-session-metadata/v1",
        session: reported,
        projectRef: null,
        repoRef: null,
        title: null,
        agentRef: null,
        provider: null,
        runtime: null,
        model: null,
        status,
        branch: null,
        capabilities: {
          threadTurnStart: true,
          threadTurnInterrupt: false,
          threadSteer: false,
          context: false,
          diff: false,
          plan: false,
        },
        sessionRef,
      }),
    },
    secret,
  );
}

/** The catalog's projection of one signed report — exactly the model's input. */
function rowFromMetadata(event, overrides = {}) {
  const content = JSON.parse(event.content);
  return {
    channelId: event.tags.find((tag) => tag[0] === "h")[1],
    targetKey: buildCodingSessionTargetKey(content.session),
    metadataEventId: event.id,
    signerPubkey: event.pubkey,
    sessionRef: content.sessionRef ?? null,
    ...overrides,
  };
}

function dispose({
  events,
  rows,
  sourceErrors = [],
  channelIds = CHANNELS,
  trustedRelayPubkey = null,
}) {
  return buildRolePackProvenance({
    events,
    channelIds,
    now: NOW,
    trustedRelayPubkey,
    sourceErrors,
    rows,
  });
}

function verdict(result, row) {
  const disposition = result.dispositions.get(rolePackProvenanceKey(row));
  assert.ok(disposition, "the row has a disposition");
  return disposition;
}

/**
 * The founder-commissioned base: genesis, create, receipt, report. Every other
 * fixture in this file is this one with exactly one fact changed.
 */
function commissionedFixture() {
  const genesis = genesisEvent({});
  const create = createEvent({
    commandId: "csl-create-1",
    genesisRef: genesis.id,
  });
  const receipt = receiptEvent({ commandId: "csl-create-1", minted: T1 });
  const report = metadataEvent({ reported: T1 });
  return {
    genesis,
    create,
    receipt,
    report,
    events: [genesis, create, receipt, report],
    row: rowFromMetadata(report),
  };
}

/** The same session resumed once by its founder, minting generation 2. */
function resumedFixture() {
  const base = commissionedFixture();
  const resume = resumeEvent({ commandId: "csl-resume-2", previous: T1 });
  const receipt = receiptEvent({
    commandId: "csl-resume-2",
    minted: T2,
    status: "resumed",
    createdAt: 1_800_000_105,
  });
  const report = metadataEvent({ reported: T2, createdAt: 1_800_000_110 });
  return {
    ...base,
    events: [...base.events, resume, receipt, report],
    resumeRow: rowFromMetadata(report),
  };
}

export {
  CHANNEL,
  OTHER_CHANNEL,
  CHANNELS,
  NOW,
  SESSION_REF,
  OTHER_SESSION_REF,
  THIRD_SESSION_REF,
  FOUNDER_SECRET,
  FOUNDER,
  PROVIDER_SECRET,
  PROVIDER,
  IMPOSTOR_X_SECRET,
  IMPOSTOR_X,
  IMPOSTOR_Y_SECRET,
  IMPOSTOR_Y,
  OPERATOR_SECRET,
  target,
  SESSION_ID,
  IMPOSTOR_SESSION_ID,
  T1,
  T2,
  T3,
  createEvent,
  resumeEvent,
  receiptEvent,
  genesisEvent,
  metadataEvent,
  rowFromMetadata,
  dispose,
  verdict,
  commissionedFixture,
  resumedFixture,
};
