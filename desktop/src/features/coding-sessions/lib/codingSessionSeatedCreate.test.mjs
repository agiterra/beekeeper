import assert from "node:assert/strict";
import test from "node:test";

import {
  codingSessionSeatPackSource,
  publishSeatedCodingSessionCreate,
  publishSeatedCodingSessionResume,
} from "./codingSessionSeatedCreate.ts";

const SEAT = {
  actor: "aa11bb22cc33dd44ee55ff66aa77bb88cc99dd00ee11ff22aa33bb44cc55dd66",
  role: "builder",
};

const OWNER = "3d3b7169".padEnd(64, "0");
const PROJECT_REF = `30621:${OWNER}:beekeeper`;
/** The project's 30624, decoded the way `fetchProjectPackSource` decodes it. */
const PACK_SOURCE_RECORD = {
  eventId: "e".repeat(64),
  author: OWNER,
  createdAt: 1_700_000_000,
  repo: `30617:${OWNER}:agiterra-packs`,
  ref: "refs/heads/main",
  sha: null,
  path: "personas/roles",
  note: null,
};
/** The same record in the shape the host's staging command takes. */
const PACK_SOURCE = {
  repo: `30617:${OWNER}:agiterra-packs`,
  gitRef: "refs/heads/main",
  sha: null,
  path: "personas/roles",
};
const PACK_REF = {
  repo: `30617:${OWNER}:agiterra-packs`,
  sha: "dd935f43".padEnd(40, "0"),
  role: "builder",
  path: "personas/roles/builder",
};

function recorder(overrides = {}) {
  const calls = [];
  return {
    calls,
    deps: {
      ensureMembership: async (input) => {
        calls.push(["ensureMembership", input]);
        if (overrides.membershipError) throw overrides.membershipError;
      },
      stageSeat: async (input) => {
        calls.push(["stageSeat", input]);
        if (overrides.stageError) throw overrides.stageError;
        return overrides.staged;
      },
      clearSeat: async (commandId) => {
        calls.push(["clearSeat", commandId]);
      },
      fetchPackSource: async (projectRef) => {
        calls.push(["fetchPackSource", projectRef]);
        if (overrides.packSourceError) throw overrides.packSourceError;
        return overrides.packSource ?? null;
      },
    },
  };
}

test("a create with no seat publishes exactly as it does today", async () => {
  const { calls, deps } = recorder();
  let published = 0;
  const result = await publishSeatedCodingSessionCreate({
    channelId: "channel-1",
    commandId: "csl-1",
    seat: null,
    projectRef: PROJECT_REF,
    deps,
    publish: async () => {
      published += 1;
      return "ok";
    },
  });
  assert.equal(result, "ok");
  assert.equal(published, 1);
  // No membership write, no custody file, no pack-source read, nothing.
  assert.deepEqual(calls, []);
});

test("the create is not published when the membership add fails", async () => {
  const { calls, deps } = recorder({
    membershipError: new Error("Could not add Ada to #crew: not permitted"),
  });
  let published = 0;
  await assert.rejects(
    publishSeatedCodingSessionCreate({
      channelId: "channel-1",
      commandId: "csl-2",
      seat: SEAT,
      seatLabel: "Ada",
      projectRef: null,
      deps,
      publish: async () => {
        published += 1;
        return "ok";
      },
    }),
    // The reason is named, not swallowed into a generic failure.
    /Could not add Ada to #crew/,
  );
  assert.equal(published, 0, "nothing may be published for a mute seat");
  // And no key material was written for a create that never went out.
  assert.deepEqual(
    calls.map(([name]) => name),
    ["ensureMembership"],
  );
});

test("the create is not published when custody staging fails", async () => {
  const { calls, deps } = recorder({
    stageError: new Error("the OS keyring may be unreachable"),
  });
  let published = 0;
  await assert.rejects(
    publishSeatedCodingSessionCreate({
      channelId: "channel-1",
      commandId: "csl-3",
      seat: SEAT,
      projectRef: null,
      deps,
      publish: async () => {
        published += 1;
        return "ok";
      },
    }),
    /keyring/,
  );
  assert.equal(published, 0);
  assert.deepEqual(
    calls.map(([name]) => name),
    ["ensureMembership", "stageSeat"],
  );
});

test("custody is staged before the publish, keyed by the exact commandId", async () => {
  const { calls, deps } = recorder();
  await publishSeatedCodingSessionCreate({
    channelId: "channel-1",
    commandId: "csl-4",
    seat: SEAT,
    seatLabel: "Ada",
    projectRef: null,
    deps,
    publish: async () => "ok",
  });
  assert.deepEqual(calls, [
    [
      "ensureMembership",
      { channelId: "channel-1", actorPubkey: SEAT.actor, actorLabel: "Ada" },
    ],
    [
      "stageSeat",
      {
        commandId: "csl-4",
        agentPubkey: SEAT.actor,
        role: "builder",
        packSource: null,
      },
    ],
  ]);
});

test("a failed publish takes the staged seat down with it", async () => {
  const { calls, deps } = recorder();
  await assert.rejects(
    publishSeatedCodingSessionCreate({
      channelId: "channel-1",
      commandId: "csl-5",
      seat: SEAT,
      projectRef: null,
      deps,
      publish: async () => {
        throw new Error("relay refused");
      },
    }),
    /relay refused/,
  );
  assert.deepEqual(calls.at(-1), ["clearSeat", "csl-5"]);
});

test("a resume stages the seat again under the resume's own commandId", async () => {
  // The create's entry was consumed when the first adapter spawned, so a
  // reconnect that stages nothing is refused `ACTOR_UNAVAILABLE` forever.
  const { calls, deps } = recorder();
  let published = 0;
  const result = await publishSeatedCodingSessionResume({
    commandId: "csl-resume-1",
    actorPubkey: SEAT.actor,
    deps,
    publish: async () => {
      published += 1;
      return "resumed";
    },
  });
  assert.equal(result, "resumed");
  assert.equal(published, 1);
  assert.deepEqual(calls, [
    [
      "stageSeat",
      {
        commandId: "csl-resume-1",
        agentPubkey: SEAT.actor,
        role: null,
        packSource: null,
      },
    ],
  ]);
});

test("a resume of an unseated execution touches no custody at all", async () => {
  const { calls, deps } = recorder();
  const result = await publishSeatedCodingSessionResume({
    commandId: "csl-resume-2",
    actorPubkey: null,
    deps,
    publish: async () => "resumed",
  });
  assert.equal(result, "resumed");
  assert.deepEqual(calls, []);
});

test("a resume that never went out takes its staged key back", async () => {
  const { calls, deps } = recorder();
  await assert.rejects(
    publishSeatedCodingSessionResume({
      commandId: "csl-resume-3",
      actorPubkey: SEAT.actor,
      deps,
      publish: async () => {
        throw new Error("relay rejected the resume");
      },
    }),
    /relay rejected the resume/,
  );
  assert.deepEqual(calls, [
    [
      "stageSeat",
      {
        commandId: "csl-resume-3",
        agentPubkey: SEAT.actor,
        role: null,
        packSource: null,
      },
    ],
    ["clearSeat", "csl-resume-3"],
  ]);
});

/**
 * What staging actually put on disk is the only place the one-session path
 * can learn that a seat is about to run without its role pack. Discarding it
 * is how the pending screen ended up quieter than the crew tab.
 */
test("staging reports what it wrote, before the publish runs", async () => {
  const order = [];
  const staged = [];
  await publishSeatedCodingSessionCreate({
    channelId: "channel-1",
    commandId: "csl-9",
    seat: SEAT,
    seatLabel: "Ada",
    projectRef: null,
    onSeatStaged: (result) => {
      order.push("onSeatStaged");
      staged.push(result);
    },
    deps: {
      ensureMembership: async () => {},
      stageSeat: async () => {
        order.push("stageSeat");
        return { packStaged: false };
      },
      clearSeat: async () => {},
      fetchPackSource: async () => null,
    },
    publish: async () => {
      order.push("publish");
      return "ok";
    },
  });
  assert.deepEqual(order, ["stageSeat", "onSeatStaged", "publish"]);
  // A backend that reported no packRef is reported as "no repository named",
  // which is `null` — not a missing key the screen has to special-case.
  assert.deepEqual(staged, [{ packStaged: false, packRef: null }]);
});

test("a resume reports its own staging too", async () => {
  const staged = [];
  await publishSeatedCodingSessionResume({
    commandId: "csl-10",
    actorPubkey: SEAT.actor,
    onSeatStaged: (result) => staged.push(result),
    deps: {
      ensureMembership: async () => {},
      stageSeat: async () => ({ packStaged: true }),
      clearSeat: async () => {},
    },
    publish: async () => "ok",
  });
  assert.deepEqual(staged, [{ packStaged: true, packRef: null }]);
});

test("nothing is reported when the create never reaches staging", async () => {
  const staged = [];
  await assert.rejects(
    publishSeatedCodingSessionCreate({
      channelId: "channel-1",
      commandId: "csl-11",
      seat: SEAT,
      projectRef: null,
      onSeatStaged: (result) => staged.push(result),
      deps: {
        ensureMembership: async () => {
          throw new Error("not a member");
        },
        stageSeat: async () => ({ packStaged: true }),
        clearSeat: async () => {},
        fetchPackSource: async () => null,
      },
      publish: async () => "ok",
    }),
    /not a member/,
  );
  assert.deepEqual(staged, []);
  // An unseated create stages nothing, so it has nothing to report either.
  await publishSeatedCodingSessionCreate({
    channelId: "channel-1",
    commandId: "csl-12",
    seat: null,
    projectRef: null,
    onSeatStaged: (result) => staged.push(result),
    deps: {
      ensureMembership: async () => {},
      stageSeat: async () => ({ packStaged: true }),
      clearSeat: async () => {},
      fetchPackSource: async () => null,
    },
    publish: async () => "ok",
  });
  assert.deepEqual(staged, []);
});

test("a backend that answers nothing is reported as unknown, not as no pack", async () => {
  const staged = [];
  await publishSeatedCodingSessionCreate({
    channelId: "channel-1",
    commandId: "csl-13",
    seat: SEAT,
    projectRef: null,
    onSeatStaged: (result) => staged.push(result),
    deps: {
      ensureMembership: async () => {},
      // An older desktop backend resolves undefined from `stage_actor_seat`.
      stageSeat: async () => undefined,
      clearSeat: async () => {},
      fetchPackSource: async () => null,
    },
    publish: async () => "ok",
  });
  assert.deepEqual(staged, []);
});

// LANE-L23: the seat's role picks the pack, so the role has to reach the host.
test("a seated create stages under the seat's role, not the actor's", async () => {
  const { calls, deps } = recorder();
  await publishSeatedCodingSessionCreate({
    channelId: "channel-1",
    commandId: "csl-role",
    seat: { actor: SEAT.actor, role: "architect" },
    projectRef: null,
    deps,
    publish: async () => "ok",
  });
  const staged = calls.find(([name]) => name === "stageSeat");
  assert.ok(staged, "the seat was staged");
  assert.deepEqual(staged[1], {
    commandId: "csl-role",
    agentPubkey: SEAT.actor,
    role: "architect",
    packSource: null,
  });
});

test("a resume restages under the same role its generation carries", async () => {
  const { calls, deps } = recorder();
  await publishSeatedCodingSessionResume({
    commandId: "csl-resume",
    actorPubkey: SEAT.actor,
    actorRole: "architect",
    deps,
    publish: async () => "ok",
  });
  const staged = calls.find(([name]) => name === "stageSeat");
  assert.ok(staged, "the seat was staged");
  assert.equal(staged[1].role, "architect");
});

test("a resume with no role named stages with none, not with a guess", async () => {
  const { calls, deps } = recorder();
  await publishSeatedCodingSessionResume({
    commandId: "csl-resume-2",
    actorPubkey: SEAT.actor,
    deps,
    publish: async () => "ok",
  });
  const staged = calls.find(([name]) => name === "stageSeat");
  assert.equal(staged[1].role, null);
});

// ── Finding 84: the project's 30624 reaches the staging call ──────────────
//
// The launch dialog's preview read the project's pack source; the create's
// staging call did not, so every seat was staged from this computer's copy
// and the repository the project pointed at was never staged from. The
// create is one function for the launch path and the hire path alike
// (`useNewCodingSessionCreate`, `useCodingSessionCrewLaunch`,
// `useCodingSessionHire` all call `publishSeatedCodingSessionCreate`), so
// what these prove holds on both.

test("finding 84: a seated create resolves the project's 30624 and stages from it", async () => {
  const { calls, deps } = recorder({
    packSource: PACK_SOURCE,
    staged: { packStaged: true, packRef: PACK_REF },
  });
  await publishSeatedCodingSessionCreate({
    channelId: "channel-1",
    commandId: "csl-84",
    seat: SEAT,
    seatLabel: "Ada",
    projectRef: PROJECT_REF,
    deps,
    publish: async () => "ok",
  });
  assert.deepEqual(
    calls.map(([name]) => name),
    ["ensureMembership", "fetchPackSource", "stageSeat"],
    "the source is read after membership and before staging",
  );
  // The reader is asked about exactly the project the create is signed with.
  assert.deepEqual(calls[1], ["fetchPackSource", PROJECT_REF]);
  // And what it answered is what the host is handed, verbatim.
  assert.deepEqual(calls[2][1], {
    commandId: "csl-84",
    agentPubkey: SEAT.actor,
    role: "builder",
    packSource: PACK_SOURCE,
  });
});

test("finding 84: a project with no 30624 stages with none — today's behaviour", async () => {
  const { calls, deps } = recorder({ packSource: null });
  await publishSeatedCodingSessionCreate({
    channelId: "channel-1",
    commandId: "csl-84-none",
    seat: SEAT,
    projectRef: PROJECT_REF,
    deps,
    publish: async () => "ok",
  });
  assert.deepEqual(calls[1], ["fetchPackSource", PROJECT_REF]);
  assert.equal(calls[2][1].packSource, null);
});

test("finding 84: a standalone create asks no project and stages with none", async () => {
  const { calls, deps } = recorder({ packSource: PACK_SOURCE });
  await publishSeatedCodingSessionCreate({
    channelId: "channel-1",
    commandId: "csl-84-standalone",
    seat: SEAT,
    projectRef: null,
    deps,
    publish: async () => "ok",
  });
  assert.ok(
    !calls.some(([name]) => name === "fetchPackSource"),
    "no project, nothing to look up",
  );
  assert.equal(calls.at(-1)[1].packSource, null);
  // Whitespace is not a project coordinate either.
  const blank = recorder({ packSource: PACK_SOURCE });
  await publishSeatedCodingSessionCreate({
    channelId: "channel-1",
    commandId: "csl-84-blank",
    seat: SEAT,
    projectRef: "   ",
    deps: blank.deps,
    publish: async () => "ok",
  });
  assert.ok(!blank.calls.some(([name]) => name === "fetchPackSource"));
});

test("finding 84: a source that cannot be read refuses the create rather than staging the wrong pack quietly", async () => {
  const { calls, deps } = recorder({
    packSourceError: new Error("relay timed out"),
  });
  let published = 0;
  await assert.rejects(
    publishSeatedCodingSessionCreate({
      channelId: "channel-1",
      commandId: "csl-84-fail",
      seat: SEAT,
      projectRef: PROJECT_REF,
      deps,
      publish: async () => {
        published += 1;
        return "ok";
      },
    }),
    (error) => {
      assert.match(error.message, /pack source \(kind 30624/);
      assert.match(error.message, new RegExp(PROJECT_REF));
      assert.match(error.message, /relay timed out/);
      return true;
    },
  );
  assert.equal(published, 0);
  // Nothing was staged, so there is nothing to clear either.
  assert.deepEqual(
    calls.map(([name]) => name),
    ["ensureMembership", "fetchPackSource"],
  );
});

test("finding 84: the report carries the packRef staging stamped, so a screen can name the commit", async () => {
  const staged = [];
  const { deps } = recorder({
    packSource: PACK_SOURCE,
    staged: { packStaged: true, packRef: PACK_REF },
  });
  await publishSeatedCodingSessionCreate({
    channelId: "channel-1",
    commandId: "csl-84-report",
    seat: SEAT,
    projectRef: PROJECT_REF,
    onSeatStaged: (result) => staged.push(result),
    deps,
    publish: async () => "ok",
  });
  assert.deepEqual(staged, [{ packStaged: true, packRef: PACK_REF }]);
});

test("finding 84: a resume passes its project's source the same way, when it has one", async () => {
  const { calls, deps } = recorder({ packSource: PACK_SOURCE });
  await publishSeatedCodingSessionResume({
    commandId: "csl-84-resume",
    actorPubkey: SEAT.actor,
    actorRole: "builder",
    projectRef: PROJECT_REF,
    deps,
    publish: async () => "ok",
  });
  assert.deepEqual(calls, [
    ["fetchPackSource", PROJECT_REF],
    [
      "stageSeat",
      {
        commandId: "csl-84-resume",
        agentPubkey: SEAT.actor,
        role: "builder",
        packSource: PACK_SOURCE,
      },
    ],
  ]);
  // A custody seam with no reader (the composer today) stages as before.
  const bare = recorder();
  delete bare.deps.fetchPackSource;
  await publishSeatedCodingSessionResume({
    commandId: "csl-84-resume-bare",
    actorPubkey: SEAT.actor,
    projectRef: PROJECT_REF,
    deps: bare.deps,
    publish: async () => "ok",
  });
  assert.equal(bare.calls.at(-1)[1].packSource, null);
});

test("finding 84: the staging shape of a 30624 is the preview's, field for field", () => {
  // `ref` becomes the host's `gitRef`; everything else rides verbatim. This
  // is the mapping `codingSessionPackStatus` and the create now share.
  assert.deepEqual(
    codingSessionSeatPackSource(PACK_SOURCE_RECORD),
    PACK_SOURCE,
  );
  assert.deepEqual(
    codingSessionSeatPackSource({
      ...PACK_SOURCE_RECORD,
      ref: null,
      sha: "a".repeat(40),
      path: "packs",
    }),
    {
      repo: PACK_SOURCE.repo,
      gitRef: null,
      sha: "a".repeat(40),
      path: "packs",
    },
  );
  assert.equal(codingSessionSeatPackSource(null), null);
});
