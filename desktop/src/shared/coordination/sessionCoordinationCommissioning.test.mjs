/**
 * Who may commission an execution — the 2026-09-05 refuter's B1, in the fold
 * the Rust rule mirrors.
 *
 * The hole the refuter executed against `buzz-core` is the same shape here:
 * the fold accepted a generation whenever the receipt was signed by the key
 * the command named, and nothing asked who signed the *command*. A seat could
 * publish a `session.create` naming itself as `providerAuthorityPubkey` — both
 * refs are public — answer it with its own receipt, and appear as a proven
 * generation of somebody else's session.
 */
import assert from "node:assert/strict";
import { test } from "node:test";

import { foldSessionCoordination } from "@/shared/coordination/sessionCoordinationFold";

const NOW = 1_785_600_000;
const CHANNEL = "05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2";
const SESSION = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const FOUNDER = "a1".repeat(32);
const PROVIDER = "d4".repeat(32);
const HOST = "b2".repeat(32);

function create({ id, commandId, signer, provider, hireRef = undefined }) {
  const action = {
    type: "session.create",
    projectRef: null,
    repoRef: null,
    sessionRef: SESSION,
    providerInstanceRef: "provider-1",
    providerAuthorityPubkey: provider,
    model: null,
    title: null,
    initialTurn: null,
    ...(hireRef === undefined ? {} : { hireRef }),
  };
  return {
    id,
    pubkey: signer,
    created_at: NOW - 100,
    kind: 44221,
    tags: [
      ["h", CHANNEL],
      ["csl-v", "csl1-1"],
      ["csl-command", commandId],
    ],
    content: JSON.stringify({
      schema: "buzz-coding-session-lifecycle-command/v1",
      commandId,
      action,
    }),
  };
}

function hire({ id, commandId, signer }) {
  return {
    id,
    pubkey: signer,
    created_at: NOW - 200,
    kind: 44221,
    tags: [
      ["h", CHANNEL],
      ["csl-v", "csl1-1"],
      ["csl-command", commandId],
    ],
    content: JSON.stringify({
      schema: "buzz-coding-session-lifecycle-command/v1",
      commandId,
      action: {
        type: "session.hire",
        sessionRef: SESSION,
        genesisRef: "cc".repeat(32),
        role: "builder",
        providerInstanceRef: null,
        model: null,
        brief: "land the slice",
        requestedBy: signer,
      },
    }),
  };
}

function receipt({ id, commandId, signer, sessionId = "sess-1" }) {
  return {
    id,
    pubkey: signer,
    created_at: NOW - 90,
    kind: 44224,
    tags: [
      ["h", CHANNEL],
      ["cslr-v", "cslr1-1"],
      ["csl-command", commandId],
      [
        "csl-key",
        `coding-session-lifecycle-receipt/v1|${commandId.length}:${commandId}`,
      ],
    ],
    content: JSON.stringify({
      schema: "buzz-coding-session-lifecycle-receipt/v1",
      commandId,
      status: "created",
      session: {
        driver: "acp",
        instanceId: "inst-1",
        sessionId,
        generation: 1,
      },
      error: null,
    }),
  };
}

test("a create its own named provider signed proves no generation", () => {
  const seat = PROVIDER;
  const fold = foldSessionCoordination({
    now: NOW,
    events: [
      create({
        id: "01".repeat(32),
        commandId: "create-1",
        signer: seat,
        provider: seat,
      }),
      receipt({ id: "02".repeat(32), commandId: "create-1", signer: seat }),
    ],
  });
  assert.deepEqual(fold.sessions, []);
  assert.equal(fold.ambiguities.length, 1);
  assert.match(
    fold.ambiguities[0].message,
    /may not commission an execution of this session/,
  );
  assert.equal(fold.ambiguities[0].scope, "authority");
});

test("the same create, signed by somebody else, proves the generation", () => {
  const fold = foldSessionCoordination({
    now: NOW,
    events: [
      create({
        id: "01".repeat(32),
        commandId: "create-1",
        signer: FOUNDER,
        provider: PROVIDER,
      }),
      receipt({ id: "02".repeat(32), commandId: "create-1", signer: PROVIDER }),
    ],
  });
  assert.equal(fold.sessions.length, 1);
  assert.deepEqual(fold.ambiguities, []);
  assert.equal(
    fold.sessions[0].generations[0].providerAuthorityPubkey,
    PROVIDER,
  );
});

test("a genuine hire cannot be borrowed by its named provider", () => {
  const asked = hire({
    id: "aa".repeat(32),
    commandId: "hire-1",
    signer: FOUNDER,
  });
  const fold = foldSessionCoordination({
    now: NOW,
    events: [
      asked,
      create({
        id: "01".repeat(32),
        commandId: "create-1",
        signer: HOST,
        provider: HOST,
        hireRef: asked.id,
      }),
      receipt({ id: "02".repeat(32), commandId: "create-1", signer: HOST }),
    ],
  });
  assert.deepEqual(fold.sessions, []);
  assert.equal(fold.ambiguities.length, 1);
});

test("a hire the provider asked itself authorizes nothing", () => {
  const asked = hire({
    id: "aa".repeat(32),
    commandId: "hire-1",
    signer: HOST,
  });
  const fold = foldSessionCoordination({
    now: NOW,
    events: [
      asked,
      create({
        id: "01".repeat(32),
        commandId: "create-1",
        signer: HOST,
        provider: HOST,
        hireRef: asked.id,
      }),
      receipt({ id: "02".repeat(32), commandId: "create-1", signer: HOST }),
    ],
  });
  assert.deepEqual(fold.sessions, []);
  assert.equal(fold.ambiguities.length, 1);
});

test("a caller that knows who steers gets the relay's own rule", () => {
  const operator = "ee".repeat(32);
  const events = [
    create({
      id: "01".repeat(32),
      commandId: "create-1",
      signer: operator,
      provider: PROVIDER,
    }),
    receipt({ id: "02".repeat(32), commandId: "create-1", signer: PROVIDER }),
  ];
  // A steering set that names the operator: accepted.
  assert.equal(
    foldSessionCoordination({
      now: NOW,
      events,
      commissioners: [FOUNDER, operator],
    }).sessions.length,
    1,
  );
  // The same events under a steering set that does not: refused, though the
  // weaker rule (signer ≠ provider) would have accepted them.
  const strict = foldSessionCoordination({
    now: NOW,
    events,
    commissioners: [FOUNDER],
  });
  assert.deepEqual(strict.sessions, []);
  assert.equal(strict.ambiguities.length, 1);
  assert.equal(
    foldSessionCoordination({ now: NOW, events }).sessions.length,
    1,
  );
});

test("a founder fulfils another seat's hire with a distinct provider", () => {
  const asked = hire({
    id: "aa".repeat(32),
    commandId: "hire-1",
    signer: HOST,
  });
  const fold = foldSessionCoordination({
    now: NOW,
    commissioners: [FOUNDER],
    events: [
      asked,
      create({
        id: "01".repeat(32),
        commandId: "create-1",
        signer: FOUNDER,
        provider: PROVIDER,
        hireRef: asked.id,
      }),
      receipt({ id: "02".repeat(32), commandId: "create-1", signer: PROVIDER }),
    ],
  });
  assert.equal(fold.sessions.length, 1, JSON.stringify(fold.ambiguities));
  assert.equal(
    fold.sessions[0].generations[0].providerAuthorityPubkey,
    PROVIDER,
  );
});

test("independently verified command IDs admit only that exact self-provider command", () => {
  const command = create({
    id: "11".repeat(32),
    commandId: "same-key",
    signer: HOST,
    provider: HOST,
  });
  const answer = receipt({
    id: "12".repeat(32),
    commandId: "same-key",
    signer: HOST,
  });
  const input = { now: NOW, events: [command, answer] };
  assert.equal(foldSessionCoordination(input).sessions.length, 0);
  assert.equal(
    foldSessionCoordination({
      ...input,
      commissionedCommandEventIds: new Set([command.id]),
    }).sessions.length,
    1,
  );
  assert.equal(
    foldSessionCoordination({
      ...input,
      commissionedCommandEventIds: new Set(["unrelated"]),
    }).sessions.length,
    0,
  );
  const conflicting = { ...command, id: "13".repeat(32) };
  assert.equal(
    foldSessionCoordination({
      ...input,
      events: [...input.events, conflicting],
      commissionedCommandEventIds: new Set([command.id, conflicting.id]),
    }).sessions.length,
    0,
  );
});
