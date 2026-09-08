import assert from "node:assert/strict";
import test from "node:test";
import {
  finalizeEvent,
  generateSecretKey,
  getPublicKey,
} from "nostr-tools/pure";
import { fetchRolePackProvenanceEvents } from "./rolePackProvenanceQuery.ts";
import {
  CHANNEL,
  OTHER_CHANNEL,
  FOUNDER_SECRET,
  OPERATOR_SECRET,
  PROVIDER_SECRET,
  PROVIDER,
  T1,
  T2,
  createEvent,
  resumeEvent,
  receiptEvent,
  metadataEvent,
  rowFromMetadata,
  commissionedFixture,
  dispose,
  verdict,
} from "./rolePackProvenanceFixtures.mjs";

const RELAY_SECRET = generateSecretKey();
const RELAY = getPublicKey(RELAY_SECRET);
const OPERATOR = getPublicKey(OPERATOR_SECRET);
const WHEN = 1_800_000_000;

function authority(genesisRef, links, channelId = CHANNEL) {
  let prevAccepted = null;
  return links.flatMap(
    (
      {
        type = "grant-operator",
        at = WHEN - 1,
        grantee = OPERATOR,
        transitionAt = at,
        relaySecret = RELAY_SECRET,
      },
      index,
    ) => {
      const seq = index + 1;
      const transition = finalizeEvent(
        {
          kind: 44228,
          created_at: transitionAt,
          tags: [
            ["h", channelId],
            ["csat-v", "csat1-1"],
            ["csat-genesis", genesisRef],
          ],
          content: JSON.stringify({
            genesisRef,
            prevAccepted,
            seq,
            type,
            granteePubkey: grantee,
          }),
        },
        FOUNDER_SECRET,
      );
      const receipt = finalizeEvent(
        {
          kind: 40099,
          created_at: at,
          tags: [["h", channelId]],
          content: JSON.stringify({
            type: "coding_session_authority_transition_accepted",
            genesisRef,
            acceptedEventId: transition.id,
            seq,
            transitionType: type,
            granteePubkey: grantee,
          }),
        },
        relaySecret,
      );
      prevAccepted = transition.id;
      return [transition, receipt];
    },
  );
}

function operatorFixture({ selfProvider = false } = {}) {
  const base = commissionedFixture();
  const providerSecret = selfProvider ? OPERATOR_SECRET : PROVIDER_SECRET;
  const provider = selfProvider ? OPERATOR : PROVIDER;
  const create = createEvent({
    secret: OPERATOR_SECRET,
    commandId: "operator-create",
    genesisRef: base.genesis.id,
    provider,
  });
  const receipt = receiptEvent({
    secret: providerSecret,
    commandId: "operator-create",
    minted: T1,
  });
  const report = metadataEvent({ secret: providerSecret, reported: T1 });
  return {
    ...base,
    events: [base.genesis, create, receipt, report],
    row: rowFromMetadata(report),
    create,
    receipt,
    report,
  };
}

for (const selfProvider of [false, true]) {
  test(`accepted operator create confirms exact lineage (operator is provider: ${selfProvider})`, async () => {
    const f = operatorFixture({ selfProvider });
    const events = [...f.events, ...authority(f.genesis.id, [{}])];
    const recovered = await fetchRolePackProvenanceEvents([CHANNEL], {
      fetchEvents: async (filter) =>
        events.filter((e) => filter.kinds.includes(e.kind)),
    });
    const result = dispose({
      ...recovered,
      rows: [f.row],
      trustedRelayPubkey: RELAY,
    });
    assert.equal(verdict(result, f.row).state, "commissioned");
    assert.equal(
      verdict(
        dispose({ events: f.events, rows: [f.row], trustedRelayPubkey: RELAY }),
        f.row,
      ).state,
      "proof-unavailable",
    );
  });
}

for (const [name, links, expected] of [
  [
    "later grant cannot authorize earlier command",
    [{ at: WHEN + 1 }],
    "proof-unavailable",
  ],
  [
    "same-second grant follows existing inclusive policy",
    [{ at: WHEN }],
    "commissioned",
  ],
  [
    "later revocation preserves older commissioning",
    [{ at: WHEN - 2 }, { type: "revoke", at: WHEN + 1 }],
    "commissioned",
  ],
  [
    "revocation before command withdraws authority",
    [{ at: WHEN - 2 }, { type: "revoke", at: WHEN - 1 }],
    "proof-unavailable",
  ],
  [
    "transition timestamp cannot replace relay acceptance time",
    [{ at: WHEN + 1, transitionAt: WHEN - 2 }],
    "proof-unavailable",
  ],
  [
    "viewer downgrade is not steering",
    [{ at: WHEN - 2 }, { type: "grant-viewer", at: WHEN - 1 }],
    "proof-unavailable",
  ],
]) {
  test(name, () => {
    const f = operatorFixture();
    const result = dispose({
      events: [...f.events, ...authority(f.genesis.id, links)],
      rows: [f.row],
      trustedRelayPubkey: RELAY,
    });
    assert.equal(verdict(result, f.row).state, expected);
  });
}

test("every resumed generation is checked and a later grant cannot repair its predecessor", () => {
  const f = operatorFixture();
  const resume = resumeEvent({
    secret: OPERATOR_SECRET,
    commandId: "operator-resume",
    previous: T1,
  });
  const receipt = receiptEvent({
    commandId: "operator-resume",
    minted: T2,
    status: "resumed",
    createdAt: WHEN + 105,
  });
  const report = metadataEvent({ reported: T2, createdAt: WHEN + 110 });
  const row = rowFromMetadata(report);
  const events = [...f.events, resume, receipt, report];
  const result = dispose({
    events: [...events, ...authority(f.genesis.id, [{ at: WHEN + 50 }])],
    rows: [row],
    trustedRelayPubkey: RELAY,
  });
  assert.equal(verdict(result, row).state, "proof-unavailable");
  const valid = dispose({
    events: [...events, ...authority(f.genesis.id, [{}])],
    rows: [row],
    trustedRelayPubkey: RELAY,
  });
  assert.equal(verdict(valid, row).state, "commissioned");
});

test("another genesis, channel or relay key cannot grant commissioning", () => {
  const f = operatorFixture();
  for (const grants of [
    authority("ab".repeat(32), [{}]),
    authority(f.genesis.id, [{}], OTHER_CHANNEL),
    authority(f.genesis.id, [{ relaySecret: OPERATOR_SECRET }]),
  ]) {
    assert.equal(
      verdict(
        dispose({
          events: [...f.events, ...grants],
          rows: [f.row],
          trustedRelayPubkey: RELAY,
        }),
        f.row,
      ).state,
      "proof-unavailable",
    );
  }
});

test("incomplete authority withdraws only operator confirmation, not founder history", () => {
  const f = operatorFixture();
  const base = commissionedFixture();
  for (const scope of ["authority-transitions", "authority-receipts"]) {
    const sourceErrors = [
      { scope, message: "History incomplete", channelIds: [CHANNEL] },
    ];
    assert.equal(
      verdict(
        dispose({
          events: [...f.events, ...authority(f.genesis.id, [{}])],
          rows: [f.row],
          trustedRelayPubkey: RELAY,
          sourceErrors,
        }),
        f.row,
      ).state,
      "proof-unavailable",
    );
    assert.equal(
      verdict(
        dispose({
          events: base.events,
          rows: [base.row],
          trustedRelayPubkey: null,
          sourceErrors,
        }),
        base.row,
      ).state,
      "commissioned",
    );
  }
});

test("verified self-provider grant does not hide a conflicting lifecycle proof", () => {
  const f = operatorFixture({ selfProvider: true });
  const other = createEvent({
    commandId: "competing",
    genesisRef: f.genesis.id,
    provider: OPERATOR,
  });
  const answer = receiptEvent({
    secret: OPERATOR_SECRET,
    commandId: "competing",
    minted: T1,
  });
  const events = [...f.events, ...authority(f.genesis.id, [{}]), other, answer];
  assert.notEqual(
    verdict(
      dispose({ events, rows: [f.row], trustedRelayPubkey: RELAY }),
      f.row,
    ).state,
    "commissioned",
  );
});

test("a founder report cannot win by dropping a competing authorized self-provider create", () => {
  const f = commissionedFixture();
  const competing = createEvent({
    secret: PROVIDER_SECRET,
    commandId: "self-competing",
    genesisRef: f.genesis.id,
    provider: PROVIDER,
  });
  const receipt = receiptEvent({ commandId: "self-competing", minted: T1 });
  const events = [
    ...f.events,
    competing,
    receipt,
    ...authority(f.genesis.id, [{ grantee: PROVIDER }]),
  ];
  assert.equal(
    verdict(
      dispose({ events, rows: [f.row], trustedRelayPubkey: RELAY }),
      f.row,
    ).state,
    "disputed",
  );
});

test("an authorized operator can resume its own provider execution", () => {
  const f = operatorFixture({ selfProvider: true });
  const resume = resumeEvent({
    secret: OPERATOR_SECRET,
    commandId: "self-resume",
    previous: T1,
    provider: OPERATOR,
  });
  const receipt = receiptEvent({
    secret: OPERATOR_SECRET,
    commandId: "self-resume",
    minted: T2,
    status: "resumed",
    createdAt: WHEN + 105,
  });
  const report = metadataEvent({
    secret: OPERATOR_SECRET,
    reported: T2,
    createdAt: WHEN + 110,
  });
  const row = rowFromMetadata(report);
  const events = [
    ...f.events,
    ...authority(f.genesis.id, [{}]),
    resume,
    receipt,
    report,
  ];
  assert.equal(
    verdict(dispose({ events, rows: [row], trustedRelayPubkey: RELAY }), row)
      .state,
    "commissioned",
  );
});

test("competing self-provider resume cannot disappear and confirm a surviving resume", () => {
  const f = commissionedFixture();
  const founderResume = resumeEvent({
    commandId: "founder-resume",
    previous: T1,
  });
  const selfResume = resumeEvent({
    secret: PROVIDER_SECRET,
    commandId: "provider-resume",
    previous: T1,
  });
  const receipts = ["founder-resume", "provider-resume"].map((commandId) =>
    receiptEvent({
      commandId,
      minted: T2,
      status: "resumed",
      createdAt: WHEN + 105,
    }),
  );
  const report = metadataEvent({ reported: T2, createdAt: WHEN + 110 });
  const row = rowFromMetadata(report);
  const events = [
    ...f.events,
    ...authority(f.genesis.id, [{ grantee: PROVIDER }]),
    founderResume,
    selfResume,
    ...receipts,
    report,
  ];
  assert.notEqual(
    verdict(dispose({ events, rows: [row], trustedRelayPubkey: RELAY }), row)
      .state,
    "commissioned",
  );
});
