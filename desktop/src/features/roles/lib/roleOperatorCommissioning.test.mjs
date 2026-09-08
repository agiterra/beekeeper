import assert from "node:assert/strict";
import test from "node:test";
import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";

import {
  authorizeRoleOperatorCommand,
  buildRoleOperatorAuthorityEvidence,
} from "./roleOperatorCommissioning.ts";

const CHANNEL_ID = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const OTHER_CHANNEL = "8f67f00c-4eef-49e5-97e7-4fbed178de4f";
const GENESIS_REF = "a1".repeat(32);
const OTHER_GENESIS = "b2".repeat(32);
const FOUNDER_SECRET = generateSecretKey();
const FOUNDER = getPublicKey(FOUNDER_SECRET);
const RELAY_SECRET = generateSecretKey();
const RELAY = getPublicKey(RELAY_SECRET);
const OPERATOR_SECRET = generateSecretKey();
const OPERATOR = getPublicKey(OPERATOR_SECRET);
const OTHER_SECRET = generateSecretKey();
const OTHER = getPublicKey(OTHER_SECRET);

function transitionEvent({
  seq,
  prevAccepted,
  type,
  granteePubkey = OPERATOR,
  role,
  channelId = CHANNEL_ID,
  genesisRef = GENESIS_REF,
  signerSecret = FOUNDER_SECRET,
  createdAt = 10,
}) {
  const content = { genesisRef, prevAccepted, seq, type, granteePubkey };
  if (role !== undefined) content.role = role;
  return finalizeEvent(
    {
      created_at: createdAt,
      kind: 44228,
      tags: [
        ["h", channelId],
        ["csat-v", "csat1-1"],
        ["csat-genesis", genesisRef],
      ],
      content: JSON.stringify(content),
    },
    signerSecret,
  );
}

function receiptEvent({
  transition,
  seq,
  transitionType,
  granteePubkey = OPERATOR,
  role,
  channelId = CHANNEL_ID,
  genesisRef = GENESIS_REF,
  signerSecret = RELAY_SECRET,
  acceptedAt,
  extraContent,
}) {
  const content = {
    type: "coding_session_authority_transition_accepted",
    genesisRef,
    acceptedEventId: transition.id,
    seq,
    transitionType,
    granteePubkey,
  };
  if (role !== undefined) content.role = role;
  if (extraContent) Object.assign(content, extraContent);
  return finalizeEvent(
    {
      created_at: acceptedAt,
      kind: 40099,
      tags: [["h", channelId]],
      content: JSON.stringify(content),
    },
    signerSecret,
  );
}

function commandEvent({
  signerSecret = OPERATOR_SECRET,
  createdAt,
  channelId = CHANNEL_ID,
}) {
  return finalizeEvent(
    {
      created_at: createdAt,
      kind: 44221,
      tags: [["h", channelId]],
      content: "{}",
    },
    signerSecret,
  );
}

function acceptedChain(specs) {
  const events = [];
  let prevAccepted = null;
  for (const [index, spec] of specs.entries()) {
    const seq = index + 1;
    const transition = transitionEvent({
      seq,
      prevAccepted,
      type: spec.type,
      granteePubkey: spec.granteePubkey ?? OPERATOR,
      role: spec.role,
      signerSecret: spec.signerSecret,
    });
    const receipt = receiptEvent({
      transition,
      seq,
      transitionType: spec.receiptType ?? spec.type,
      granteePubkey: spec.receiptGrantee ?? spec.granteePubkey ?? OPERATOR,
      role: Object.hasOwn(spec, "receiptRole") ? spec.receiptRole : spec.role,
      acceptedAt: spec.acceptedAt,
    });
    events.push(transition, receipt);
    prevAccepted = transition.id;
  }
  return events;
}

function buildEvidence(value = {}) {
  return buildRoleOperatorAuthorityEvidence({
    events: value.events ?? [],
    trustedRelayPubkey: Object.hasOwn(value, "trustedRelayPubkey")
      ? value.trustedRelayPubkey
      : RELAY,
    scope: value.scope ?? {
      channelId: CHANNEL_ID,
      genesisRef: GENESIS_REF,
      founderPubkey: FOUNDER,
    },
    sourceComplete: value.sourceComplete ?? true,
  });
}

test("builds one reusable exact receipt-backed timeline", () => {
  const events = acceptedChain([
    { type: "grant-operator", acceptedAt: 100 },
    { type: "revoke", acceptedAt: 102 },
  ]);
  const evidence = buildEvidence({ events });

  assert.equal(evidence.disposition, "complete");
  assert.equal(evidence.reason, null);
  assert.deepEqual(
    evidence.timeline.map((link) => ({
      seq: link.seq,
      type: link.type,
      transitionEventId: link.transitionEventId,
      receiptEventId: link.receiptEventId,
      acceptedAt: link.acceptedAt,
    })),
    [
      {
        seq: 1,
        type: "grant-operator",
        transitionEventId: events[0].id,
        receiptEventId: events[1].id,
        acceptedAt: 100,
      },
      {
        seq: 2,
        type: "revoke",
        transitionEventId: events[2].id,
        receiptEventId: events[3].id,
        acceptedAt: 102,
      },
    ],
  );
});

test("receipt time is inclusive and transition time confers no authority", () => {
  const events = acceptedChain([{ type: "grant-operator", acceptedAt: 101 }]);
  const evidence = buildEvidence({ events });

  assert.equal(
    authorizeRoleOperatorCommand(evidence, commandEvent({ createdAt: 100 }))
      .authorized,
    false,
  );
  assert.deepEqual(
    authorizeRoleOperatorCommand(evidence, commandEvent({ createdAt: 101 })),
    { authorized: true, basis: "operator", reason: null },
  );
});

test("later revoke and viewer downgrade preserve only earlier commissions", () => {
  for (const disablingType of ["revoke", "grant-viewer"]) {
    const evidence = buildEvidence({
      events: acceptedChain([
        { type: "grant-operator", acceptedAt: 100 },
        { type: disablingType, acceptedAt: 102 },
      ]),
    });
    assert.equal(
      authorizeRoleOperatorCommand(evidence, commandEvent({ createdAt: 101 }))
        .authorized,
      true,
    );
    assert.equal(
      authorizeRoleOperatorCommand(evidence, commandEvent({ createdAt: 103 }))
        .authorized,
      false,
    );
  }
});

test("seat grants and a lead role never confer steering authority", () => {
  const evidence = buildEvidence({
    events: acceptedChain([
      { type: "grant-seat", role: "lead", acceptedAt: 100 },
    ]),
  });
  assert.equal(evidence.disposition, "complete");
  assert.equal(
    authorizeRoleOperatorCommand(evidence, commandEvent({ createdAt: 101 }))
      .authorized,
    false,
  );
});

test("nonmonotonic receipt times are evaluated in chain order", () => {
  const evidence = buildEvidence({
    events: acceptedChain([
      { type: "grant-viewer", acceptedAt: 200 },
      { type: "grant-operator", acceptedAt: 100 },
    ]),
  });
  assert.equal(
    authorizeRoleOperatorCommand(evidence, commandEvent({ createdAt: 150 }))
      .authorized,
    true,
  );
  assert.equal(
    authorizeRoleOperatorCommand(evidence, commandEvent({ createdAt: 200 }))
      .authorized,
    true,
  );
});

test("founder authority survives relay-key and authority-read failures", () => {
  const command = commandEvent({
    signerSecret: FOUNDER_SECRET,
    createdAt: 50,
  });
  for (const evidence of [
    buildEvidence({ trustedRelayPubkey: null }),
    buildEvidence({ sourceComplete: false }),
  ]) {
    assert.equal(evidence.disposition, "incomplete");
    assert.deepEqual(authorizeRoleOperatorCommand(evidence, command), {
      authorized: true,
      basis: "founder",
      reason: null,
    });
  }
});

test("wrong relay, channel, and genesis evidence grants nothing", () => {
  const transition = transitionEvent({
    seq: 1,
    prevAccepted: null,
    type: "grant-operator",
  });
  const attacks = [
    receiptEvent({
      transition,
      seq: 1,
      transitionType: "grant-operator",
      acceptedAt: 100,
      signerSecret: OTHER_SECRET,
    }),
    receiptEvent({
      transition,
      seq: 1,
      transitionType: "grant-operator",
      acceptedAt: 100,
      channelId: OTHER_CHANNEL,
    }),
    receiptEvent({
      transition,
      seq: 1,
      transitionType: "grant-operator",
      acceptedAt: 100,
      genesisRef: OTHER_GENESIS,
    }),
  ];
  for (const receipt of attacks) {
    const evidence = buildEvidence({ events: [transition, receipt] });
    assert.equal(
      authorizeRoleOperatorCommand(evidence, commandEvent({ createdAt: 101 }))
        .authorized,
      false,
    );
  }
});

test("receipt fact mismatch and non-founder legacy links are conflicted", () => {
  const mismatch = acceptedChain([
    {
      type: "grant-operator",
      receiptType: "grant-viewer",
      acceptedAt: 100,
    },
  ]);
  assert.equal(buildEvidence({ events: mismatch }).disposition, "conflicted");

  const nonFounder = acceptedChain([
    {
      type: "grant-operator",
      acceptedAt: 100,
      signerSecret: OTHER_SECRET,
    },
  ]);
  assert.equal(buildEvidence({ events: nonFounder }).disposition, "conflicted");
});

test("missing predecessor and conflicting sequence expose no authority", () => {
  const first = transitionEvent({
    seq: 1,
    prevAccepted: null,
    type: "grant-operator",
  });
  const second = transitionEvent({
    seq: 2,
    prevAccepted: OTHER_GENESIS,
    type: "grant-operator",
  });
  const events = [
    first,
    receiptEvent({
      transition: first,
      seq: 1,
      transitionType: "grant-operator",
      acceptedAt: 100,
    }),
    second,
    receiptEvent({
      transition: second,
      seq: 2,
      transitionType: "grant-operator",
      acceptedAt: 101,
    }),
  ];
  const evidence = buildEvidence({ events });
  assert.equal(evidence.disposition, "conflicted");
  assert.equal(
    authorizeRoleOperatorCommand(evidence, commandEvent({ createdAt: 102 }))
      .authorized,
    false,
  );

  const competing = transitionEvent({
    seq: 1,
    prevAccepted: null,
    type: "grant-operator",
    granteePubkey: OTHER,
  });
  const conflict = buildEvidence({
    events: [
      first,
      events[1],
      competing,
      receiptEvent({
        transition: competing,
        seq: 1,
        transitionType: "grant-operator",
        granteePubkey: OTHER,
        acceptedAt: 100,
      }),
    ],
  });
  assert.equal(conflict.disposition, "conflicted");
});

test("missing accepted transition is incomplete; identical event IDs deduplicate", () => {
  const events = acceptedChain([{ type: "grant-operator", acceptedAt: 100 }]);
  const missing = buildEvidence({ events: [events[1]] });
  assert.equal(missing.disposition, "incomplete");

  const malformed = receiptEvent({
    transition: events[0],
    seq: 1,
    transitionType: "grant-operator",
    acceptedAt: 100,
    extraContent: { unboundAuthority: true },
  });
  assert.equal(
    buildEvidence({ events: [events[0], malformed] }).disposition,
    "incomplete",
  );

  const duplicateDelivery = buildEvidence({
    events: [events[0], events[0], events[1], events[1]],
  });
  assert.equal(duplicateDelivery.disposition, "complete");
  assert.equal(duplicateDelivery.timeline.length, 1);

  const secondReceipt = receiptEvent({
    transition: events[0],
    seq: 1,
    transitionType: "grant-operator",
    acceptedAt: 101,
  });
  assert.equal(
    buildEvidence({ events: [...events, secondReceipt] }).disposition,
    "conflicted",
  );
});

test("forged authority bytes and malformed commands cannot authorize", () => {
  const events = acceptedChain([{ type: "grant-operator", acceptedAt: 100 }]);
  const forgedReceipt = {
    ...events[1],
    content: `${events[1].content} `,
  };
  assert.equal(
    authorizeRoleOperatorCommand(
      buildEvidence({ events: [events[0], forgedReceipt] }),
      commandEvent({ createdAt: 101 }),
    ).authorized,
    false,
  );

  const evidence = buildEvidence({ events });
  const wrongChannel = commandEvent({
    createdAt: 101,
    channelId: OTHER_CHANNEL,
  });
  assert.equal(
    authorizeRoleOperatorCommand(evidence, wrongChannel).authorized,
    false,
  );
  assert.equal(
    authorizeRoleOperatorCommand(evidence, {
      ...commandEvent({ createdAt: 101 }),
      content: "forged",
    }).authorized,
    false,
  );
});
