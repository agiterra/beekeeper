/**
 * Provenance for reported pack revisions, proved against real signed bytes.
 *
 * Every fixture here is finalized with nostr-tools and verified by the same
 * classifiers the app runs, because the whole claim under test is about *who
 * signed what*. A hand-rolled event object would let the suite pass while the
 * product's signature gates were wrong, which is the failure mode this module
 * exists to prevent.
 *
 * The reported rows are built from real kind-44223 events through
 * {@link rowFromMetadata}, so the channel, target, signer and echoed session a
 * row carries are the ones a signed report actually carries — the model itself
 * takes rows rather than 44223 bytes, since the catalog upstream has already
 * verified and projected them.
 */
import assert from "node:assert/strict";
import test from "node:test";

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
  decodeCodingSessionTargetKey,
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

function dispose({ events, rows, sourceErrors = [], channelIds = CHANNELS }) {
  return buildRolePackProvenance({
    events,
    channelIds,
    now: NOW,
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

test("a founder-signed create commissions the provider that reported the pack", () => {
  const fixture = commissionedFixture();
  const result = dispose({ events: fixture.events, rows: [fixture.row] });
  const disposition = verdict(result, fixture.row);

  assert.equal(disposition.state, "commissioned");
  assert.equal(disposition.reason, null);
  assert.equal(disposition.founderPubkey, FOUNDER);
  assert.equal(disposition.commandSignerPubkey, FOUNDER);
  assert.equal(disposition.commandEventId, fixture.create.id);
  assert.equal(disposition.receiptEventId, fixture.receipt.id);
  assert.equal(disposition.genesisEventId, fixture.genesis.id);
  assert.deepEqual(result.notes, []);
});

test("a founder-signed resume commissions generation 2 on its own proof", () => {
  const fixture = resumedFixture();
  const result = dispose({
    events: fixture.events,
    rows: [fixture.row, fixture.resumeRow],
  });

  assert.equal(verdict(result, fixture.row).state, "commissioned");
  const resumed = verdict(result, fixture.resumeRow);
  assert.equal(resumed.state, "commissioned");
  assert.equal(resumed.founderPubkey, FOUNDER);
});

test("a report for a generation with no lifecycle pair inherits nothing", () => {
  const fixture = resumedFixture();
  const orphan = metadataEvent({ reported: T3, createdAt: 1_800_000_210 });
  const row = rowFromMetadata(orphan);
  const result = dispose({
    events: [...fixture.events, orphan],
    rows: [fixture.resumeRow, row],
  });

  assert.equal(verdict(result, fixture.resumeRow).state, "commissioned");
  const disposition = verdict(result, row);
  assert.equal(disposition.state, "proof-unavailable");
  assert.equal(
    disposition.reason,
    "No accepted lifecycle proof for this generation.",
  );
  assert.equal(disposition.commandEventId, null);
});

test("an impostor create citing the founder's genesis is proof-unavailable, not disputed", () => {
  const base = commissionedFixture();
  const impostorTarget = target(IMPOSTOR_SESSION_ID, 1);
  const create = createEvent({
    secret: IMPOSTOR_X_SECRET,
    commandId: "csl-impostor-1",
    genesisRef: base.genesis.id,
    provider: IMPOSTOR_Y,
  });
  const receipt = receiptEvent({
    secret: IMPOSTOR_Y_SECRET,
    commandId: "csl-impostor-1",
    minted: impostorTarget,
  });
  const report = metadataEvent({
    secret: IMPOSTOR_Y_SECRET,
    reported: impostorTarget,
  });
  const row = rowFromMetadata(report);
  const result = dispose({
    events: [...base.events, create, receipt, report],
    rows: [base.row, row],
  });

  assert.equal(verdict(result, base.row).state, "commissioned");
  const disposition = verdict(result, row);
  assert.equal(disposition.state, "proof-unavailable");
  assert.match(disposition.reason, /is not the founder/);
  assert.match(disposition.reason, /operator authority is not projected yet/);
  assert.equal(disposition.commandSignerPubkey, IMPOSTOR_X);
  assert.equal(disposition.founderPubkey, FOUNDER);
});

test("a report signed by a key that is not this generation's provider is disputed", () => {
  const base = commissionedFixture();
  const forged = metadataEvent({
    secret: IMPOSTOR_Y_SECRET,
    reported: T1,
    createdAt: 1_800_000_020,
  });
  const row = rowFromMetadata(forged);
  const result = dispose({
    events: [...base.events, forged],
    rows: [base.row, row],
  });

  const disposition = verdict(result, row);
  assert.equal(disposition.state, "disputed");
  assert.equal(
    disposition.reason,
    "This report is signed by a key that is not the provider this generation was commissioned from.",
  );
  assert.equal(verdict(result, base.row).state, "commissioned");
});

test("an operator resume leaves the resumed generation's report proof-unavailable", () => {
  const base = commissionedFixture();
  const resume = resumeEvent({
    secret: OPERATOR_SECRET,
    commandId: "csl-operator-2",
    previous: T1,
  });
  const receipt = receiptEvent({
    commandId: "csl-operator-2",
    minted: T2,
    status: "resumed",
    createdAt: 1_800_000_105,
  });
  const report = metadataEvent({ reported: T2, createdAt: 1_800_000_110 });
  const row = rowFromMetadata(report);
  const result = dispose({
    events: [...base.events, resume, receipt, report],
    rows: [base.row, row],
  });

  assert.equal(verdict(result, base.row).state, "commissioned");
  const disposition = verdict(result, row);
  assert.equal(disposition.state, "proof-unavailable");
  assert.equal(
    disposition.reason,
    "The command that started generation 2 was signed by a key that is not the founder; operator authority is not projected yet, so this cannot be read as a grant.",
  );
});

test("a genesis published in another channel contradicts the create and is disputed", () => {
  const genesis = genesisEvent({
    channelId: OTHER_CHANNEL,
    sessionRef: OTHER_SESSION_REF,
  });
  const create = createEvent({
    commandId: "csl-crosschannel-1",
    genesisRef: genesis.id,
    sessionRef: OTHER_SESSION_REF,
  });
  const minted = target("33333333-3333-3333-3333-333333333333", 1);
  const receipt = receiptEvent({
    commandId: "csl-crosschannel-1",
    minted,
  });
  const report = metadataEvent({
    reported: minted,
    sessionRef: OTHER_SESSION_REF,
  });
  const row = rowFromMetadata(report);
  const result = dispose({
    events: [genesis, create, receipt, report],
    rows: [row],
  });

  const disposition = verdict(result, row);
  assert.equal(disposition.state, "disputed");
  assert.equal(
    disposition.reason,
    "The genesis this create cites was published in a different channel than the create.",
  );
  assert.equal(disposition.genesisEventId, genesis.id);
});

test("a create and the genesis it cites naming different sessions is disputed", () => {
  const genesis = genesisEvent({ sessionRef: THIRD_SESSION_REF });
  const create = createEvent({
    commandId: "csl-crossed-session",
    genesisRef: genesis.id,
    sessionRef: OTHER_SESSION_REF,
  });
  const minted = target("44444444-4444-4444-4444-444444444444", 1);
  const receipt = receiptEvent({ commandId: "csl-crossed-session", minted });
  const report = metadataEvent({
    reported: minted,
    sessionRef: OTHER_SESSION_REF,
  });
  const row = rowFromMetadata(report);
  const result = dispose({
    events: [genesis, create, receipt, report],
    rows: [row],
  });

  const disposition = verdict(result, row);
  assert.equal(disposition.state, "disputed");
  assert.equal(
    disposition.reason,
    "The create and the genesis it cites name different sessions.",
  );
});

test("a report echoing a different session than the commissioned one is disputed", () => {
  const fixture = commissionedFixture();
  const row = rowFromMetadata(fixture.report, {
    sessionRef: OTHER_SESSION_REF,
  });
  const result = dispose({ events: fixture.events, rows: [row] });

  const disposition = verdict(result, row);
  assert.equal(disposition.state, "disputed");
  assert.equal(
    disposition.reason,
    "This report echoes a different session than the one this generation was commissioned for.",
  );
});

test("a report that names no session is proof-unavailable, never commissioned", () => {
  const fixture = commissionedFixture();
  const row = rowFromMetadata(fixture.report, { sessionRef: null });
  const result = dispose({ events: fixture.events, rows: [row] });

  const disposition = verdict(result, row);
  assert.equal(disposition.state, "proof-unavailable");
  assert.equal(disposition.reason, "This report names no session.");
  assert.equal(disposition.founderPubkey, FOUNDER);
});

test("two lifecycle proofs claiming one target leave the report disputed", () => {
  const genesis = genesisEvent({});
  const minted = target("55555555-5555-5555-5555-555555555555", 1);
  const first = createEvent({
    commandId: "csl-twin-a",
    genesisRef: genesis.id,
  });
  const firstReceipt = receiptEvent({ commandId: "csl-twin-a", minted });
  const second = createEvent({
    commandId: "csl-twin-b",
    genesisRef: genesis.id,
    createdAt: 1_800_000_001,
  });
  const secondReceipt = receiptEvent({
    commandId: "csl-twin-b",
    minted,
    createdAt: 1_800_000_006,
  });
  const report = metadataEvent({ reported: minted });
  const row = rowFromMetadata(report);
  const result = dispose({
    events: [genesis, first, firstReceipt, second, secondReceipt, report],
    rows: [row],
  });

  const disposition = verdict(result, row);
  assert.equal(disposition.state, "disputed");
  assert.equal(
    disposition.reason,
    "More than one lifecycle proof claims this generation, so none was accepted.",
  );
  assert.ok(
    result.notes.some((note) =>
      note.startsWith("Generation 1 of session 55555555"),
    ),
    `notes disclose the contested proof: ${JSON.stringify(result.notes)}`,
  );
});

test("a create that names no genesis can prove nothing about a founder", () => {
  const create = createEvent({ commandId: "csl-no-genesis" });
  const minted = target("66666666-6666-6666-6666-666666666666", 1);
  const receipt = receiptEvent({ commandId: "csl-no-genesis", minted });
  const report = metadataEvent({ reported: minted });
  const row = rowFromMetadata(report);
  const result = dispose({ events: [create, receipt, report], rows: [row] });

  const disposition = verdict(result, row);
  assert.equal(disposition.state, "proof-unavailable");
  assert.equal(
    disposition.reason,
    "This generation's create names no genesis.",
  );
  assert.equal(disposition.founderPubkey, null);
});

test("a genesis this build never received is proof-unavailable, not disputed", () => {
  const genesis = genesisEvent({});
  const create = createEvent({
    commandId: "csl-absent-genesis",
    genesisRef: genesis.id,
  });
  const minted = target("77777777-7777-7777-7777-777777777777", 1);
  const receipt = receiptEvent({ commandId: "csl-absent-genesis", minted });
  const report = metadataEvent({ reported: minted });
  const row = rowFromMetadata(report);
  const result = dispose({ events: [create, receipt, report], rows: [row] });

  const disposition = verdict(result, row);
  assert.equal(disposition.state, "proof-unavailable");
  assert.equal(
    disposition.reason,
    "The genesis this create cites could not be read.",
  );
});

/**
 * A create answering a 44221 hire carries `hireRef` — attribution only. The
 * founder fulfilling a lead's hire signs it like any other create, so the
 * generation it opens is commissioned on exactly the same terms; `hireRef`
 * neither adds nor removes authority. (The classifier once refused the key
 * and every founder-fulfilled hire read as unreadable; this pins the repair.)
 */
test("a founder-signed create that answers a hire is commissioned on the same terms", () => {
  const genesis = genesisEvent({});
  const create = createEvent({
    commandId: "csl-hired-1",
    genesisRef: genesis.id,
    hireRef: "ab".repeat(32),
  });
  const minted = target("88888888-8888-8888-8888-888888888888", 1);
  const receipt = receiptEvent({ commandId: "csl-hired-1", minted });
  const report = metadataEvent({ reported: minted });
  const row = rowFromMetadata(report);
  const result = dispose({
    events: [genesis, create, receipt, report],
    rows: [row],
  });

  const disposition = verdict(result, row);
  assert.equal(disposition.state, "commissioned");
  assert.equal(disposition.reason, null);
  assert.equal(disposition.commandEventId, create.id);
  assert.equal(disposition.genesisEventId, genesis.id);
});

test("a source error is carried verbatim into every unproven row and the notes", () => {
  const fixture = commissionedFixture();
  const orphan = metadataEvent({ reported: T2, createdAt: 1_800_000_210 });
  const row = rowFromMetadata(orphan);
  const sentence = "Lifecycle receipts could not be read: the relay timed out.";
  const result = dispose({
    events: [...fixture.events, orphan],
    rows: [row],
    sourceErrors: [{ scope: "lifecycle-receipts", message: sentence }],
  });

  const disposition = verdict(result, row);
  assert.equal(disposition.state, "proof-unavailable");
  assert.equal(
    disposition.reason,
    `No accepted lifecycle proof for this generation. ${sentence}`,
  );
  assert.deepEqual(result.notes, [sentence]);
});

/**
 * The page that did not arrive is exactly where an older conflicting command
 * would be, so a positive earned over a truncated proof read is a positive
 * this module cannot stand behind.
 */
test("a truncated proof read suppresses the positive in the channel it hit", () => {
  const fixture = commissionedFixture();
  const truncation =
    "Lifecycle commands were truncated at 1000 events; older proof may be missing.";
  const result = dispose({
    events: fixture.events,
    rows: [fixture.row],
    sourceErrors: [
      {
        scope: "lifecycle-commands",
        message: truncation,
        channelIds: [CHANNEL],
      },
    ],
  });

  const disposition = verdict(result, fixture.row);
  assert.equal(disposition.state, "proof-unavailable");
  assert.equal(
    disposition.reason,
    `Proof reads for this channel were incomplete, so this report cannot be confirmed: ${truncation}`,
  );
  // The evidence that *was* read is still reported, so a reader can see how
  // close the row came and which genesis it was bound to.
  assert.equal(disposition.founderPubkey, FOUNDER);
  assert.equal(disposition.genesisEventId, fixture.genesis.id);
});

test("an incomplete read in one channel leaves another channel's positive alone", () => {
  const genesis = genesisEvent({
    channelId: OTHER_CHANNEL,
    sessionRef: OTHER_SESSION_REF,
  });
  const create = createEvent({
    commandId: "csl-other-channel",
    channelId: OTHER_CHANNEL,
    sessionRef: OTHER_SESSION_REF,
    genesisRef: genesis.id,
  });
  const minted = target("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", 1);
  const receipt = receiptEvent({
    commandId: "csl-other-channel",
    channelId: OTHER_CHANNEL,
    minted,
  });
  const report = metadataEvent({
    channelId: OTHER_CHANNEL,
    reported: minted,
    sessionRef: OTHER_SESSION_REF,
  });
  const unaffected = rowFromMetadata(report);
  const fixture = commissionedFixture();
  const result = dispose({
    events: [...fixture.events, genesis, create, receipt, report],
    rows: [fixture.row, unaffected],
    sourceErrors: [
      {
        scope: "lifecycle-receipts",
        message: "Lifecycle receipts could not be read: the relay timed out.",
        channelIds: [CHANNEL],
      },
    ],
  });

  assert.equal(verdict(result, fixture.row).state, "proof-unavailable");
  assert.equal(verdict(result, unaffected).state, "commissioned");
});

test("a failure the reader cannot localise suppresses every channel's positive", () => {
  const fixture = commissionedFixture();
  const result = dispose({
    events: fixture.events,
    rows: [fixture.row],
    sourceErrors: [
      {
        scope: "session-genesis",
        message: "Session genesis records could not be read: offline.",
      },
    ],
  });

  assert.equal(verdict(result, fixture.row).state, "proof-unavailable");
  assert.match(verdict(result, fixture.row).reason, /^Proof reads for this/);
});

test("a report read that failed suppresses nothing — it hides rows, not proof", () => {
  const fixture = commissionedFixture();
  const result = dispose({
    events: fixture.events,
    rows: [fixture.row],
    sourceErrors: [
      {
        scope: "session-reports",
        message: "Session reports were truncated at 1000 events.",
        channelIds: [CHANNEL],
      },
    ],
  });

  assert.equal(verdict(result, fixture.row).state, "commissioned");
});

test("an incomplete read never turns a contradiction back into a maybe", () => {
  const base = commissionedFixture();
  const forged = metadataEvent({
    secret: IMPOSTOR_Y_SECRET,
    reported: T1,
    createdAt: 1_800_000_020,
  });
  const row = rowFromMetadata(forged);
  const result = dispose({
    events: [...base.events, forged],
    rows: [row],
    sourceErrors: [
      {
        scope: "lifecycle-commands",
        message: "Lifecycle commands were truncated at 1000 events.",
        channelIds: [CHANNEL],
      },
    ],
  });

  assert.equal(verdict(result, row).state, "disputed");
});

test("two reports of one target from two signers keep two dispositions", () => {
  const base = commissionedFixture();
  const forged = metadataEvent({
    secret: IMPOSTOR_Y_SECRET,
    reported: T1,
    createdAt: 1_800_000_020,
  });
  const forgedRow = rowFromMetadata(forged);
  const result = dispose({
    events: [...base.events, forged],
    rows: [base.row, forgedRow],
  });

  assert.equal(result.dispositions.size, 2);
  assert.notEqual(
    rolePackProvenanceKey(base.row),
    rolePackProvenanceKey(forgedRow),
  );
  assert.equal(verdict(result, base.row).state, "commissioned");
  assert.equal(verdict(result, forgedRow).state, "disputed");
});

test("a row with no target key is proof-unavailable and says which fact is missing", () => {
  const fixture = commissionedFixture();
  const row = rowFromMetadata(fixture.report, { targetKey: null });
  const result = dispose({ events: fixture.events, rows: [row] });

  const disposition = verdict(result, row);
  assert.equal(disposition.state, "proof-unavailable");
  assert.equal(disposition.reason, "This report names no execution target.");
});

test("fold refusals become sentences that dump no pubkey", () => {
  const genesis = genesisEvent({});
  // A self-certifying create: the signer names itself as the provider, so the
  // fold refuses it and accepts no generation at all.
  const create = createEvent({
    secret: PROVIDER_SECRET,
    commandId: "csl-self",
    genesisRef: genesis.id,
    provider: PROVIDER,
  });
  const minted = target("99999999-9999-9999-9999-999999999999", 1);
  const receipt = receiptEvent({ commandId: "csl-self", minted });
  const report = metadataEvent({ reported: minted });
  const row = rowFromMetadata(report);
  const result = dispose({
    events: [genesis, create, receipt, report],
    rows: [row],
  });

  assert.equal(verdict(result, row).state, "proof-unavailable");
  assert.deepEqual(result.notes, [
    "A lifecycle command (csl-self) was signed by a key that may not commission this session, so no generation was proven from it.",
  ]);
  for (const note of result.notes) {
    assert.doesNotMatch(note, /[0-9a-f]{64}/);
  }
});

test("the disposition key keeps channel, target, metadata event and signer apart", () => {
  const row = {
    channelId: CHANNEL,
    targetKey: buildCodingSessionTargetKey(T1),
    metadataEventId: "cd".repeat(32),
    signerPubkey: PROVIDER,
    sessionRef: SESSION_REF,
  };
  assert.equal(
    rolePackProvenanceKey(row),
    `${CHANNEL}|${buildCodingSessionTargetKey(T1)}|${"cd".repeat(32)}|${PROVIDER}`,
  );
  assert.equal(
    rolePackProvenanceKey({
      channelId: CHANNEL,
      targetKey: null,
      metadataEventId: null,
      signerPubkey: null,
      sessionRef: null,
    }),
    `${CHANNEL}|||`,
  );
});

test("a target key decodes back to the tuple a refusal has to name", () => {
  assert.deepEqual(
    decodeCodingSessionTargetKey(buildCodingSessionTargetKey(T3)),
    T3,
  );
  assert.equal(decodeCodingSessionTargetKey("coding-session/v1|nope"), null);
  assert.equal(decodeCodingSessionTargetKey("other/v1|1:a"), null);
  // A field whose own bytes look like a length prefix still decodes exactly.
  const tricky = target("3:x", 2);
  assert.deepEqual(
    decodeCodingSessionTargetKey(buildCodingSessionTargetKey(tricky)),
    tricky,
  );
});
