import assert from "node:assert/strict";
import test from "node:test";

import {
  codingSessionSeatRoleNotice,
  defaultCodingSessionSeatRole,
  isCodingSessionRoleSlug,
  normalizeCodingSessionRoleSlug,
  resolveCodingSessionActorSeat,
} from "./codingSessionActorSeat.ts";
import { buildCodingSessionCreateEvent } from "./codingSessionLifecycleCommand.ts";
import { parseBuzzCodingSessionMetadata } from "./codingSessionIngressPayloads.ts";

const ACTOR =
  "aa11bb22cc33dd44ee55ff66aa77bb88cc99dd00ee11ff22aa33bb44cc55dd66";
const PROVIDER = "b".repeat(64);

test("a role slug is lowercase letters, digits and hyphens", () => {
  assert.ok(isCodingSessionRoleSlug("lead"));
  assert.ok(isCodingSessionRoleSlug("code-reviewer-2"));
  assert.ok(!isCodingSessionRoleSlug("Lead"));
  assert.ok(!isCodingSessionRoleSlug("code reviewer"));
  assert.ok(!isCodingSessionRoleSlug(""));
  assert.ok(!isCodingSessionRoleSlug("a".repeat(65)));
  assert.ok(isCodingSessionRoleSlug("a".repeat(64)));
});

test("typed role text folds into a slug, or into nothing", () => {
  assert.equal(normalizeCodingSessionRoleSlug("  Lead  "), "lead");
  assert.equal(
    normalizeCodingSessionRoleSlug("Code Reviewer"),
    "code-reviewer",
  );
  assert.equal(normalizeCodingSessionRoleSlug("--builder--"), "builder");
  assert.equal(normalizeCodingSessionRoleSlug("naïve"), null);
  assert.equal(normalizeCodingSessionRoleSlug("   "), null);
});

test("an empty seat is an ordinary unseated create, not an error", () => {
  assert.deepEqual(resolveCodingSessionActorSeat({ actor: null, role: "" }), {
    seat: null,
    error: null,
  });
});

test("half a seat is refused with a reason a person can act on", () => {
  const noRole = resolveCodingSessionActorSeat({ actor: ACTOR, role: "  " });
  assert.equal(noRole.seat, null);
  assert.match(noRole.error, /role/i);
  const noActor = resolveCodingSessionActorSeat({ actor: "", role: "lead" });
  assert.equal(noActor.seat, null);
  assert.match(noActor.error, /agent/i);
});

test("a whole seat normalizes both halves", () => {
  assert.deepEqual(
    resolveCodingSessionActorSeat({
      actor: ACTOR.toUpperCase(),
      role: " Builder ",
    }),
    { seat: { actor: ACTOR, role: "builder" }, error: null },
  );
});

function createInput(extra) {
  return {
    channelId: "channel-1",
    commandId: "csl-1",
    projectRef: null,
    repoRef: null,
    providerInstanceRef: "codex-primary",
    providerAuthorityPubkey: PROVIDER,
    model: null,
    title: null,
    initialTurn: null,
    ...extra,
  };
}

test("an unseated create's bytes are unchanged by this feature", () => {
  const event = buildCodingSessionCreateEvent(createInput());
  const action = JSON.parse(event.content).action;
  assert.deepEqual(Object.keys(action), [
    "type",
    "projectRef",
    "repoRef",
    "providerInstanceRef",
    "providerAuthorityPubkey",
    "model",
    "title",
    "initialTurn",
  ]);
});

test("a seated create carries actor and role, and only as a pair", () => {
  const event = buildCodingSessionCreateEvent(
    createInput({ actor: ACTOR, role: "builder" }),
  );
  const action = JSON.parse(event.content).action;
  assert.equal(action.actor, ACTOR);
  assert.equal(action.role, "builder");
  assert.throws(
    () => buildCodingSessionCreateEvent(createInput({ actor: ACTOR })),
    /together/,
  );
  assert.throws(
    () => buildCodingSessionCreateEvent(createInput({ role: "builder" })),
    /together/,
  );
  assert.throws(
    () =>
      buildCodingSessionCreateEvent(
        createInput({ actor: ACTOR.toUpperCase(), role: "builder" }),
      ),
    /lowercase 64-hex/,
  );
  assert.throws(
    () =>
      buildCodingSessionCreateEvent(
        createInput({ actor: ACTOR, role: "Builder" }),
      ),
    /a-z0-9/,
  );
});

function metadata(extra) {
  return JSON.stringify({
    schema: "buzz-coding-session-metadata/v1",
    session: {
      driver: "codex-acp",
      instanceId: "instance-1",
      sessionId: "session-1",
      generation: 1,
    },
    projectRef: null,
    repoRef: null,
    title: null,
    agentRef: null,
    provider: null,
    runtime: "codex-acp",
    model: null,
    status: "idle",
    branch: null,
    capabilities: {
      threadTurnStart: true,
      threadTurnInterrupt: false,
      threadSteer: false,
      context: false,
      diff: false,
      plan: false,
    },
    ...extra,
  });
}

test("metadata with no actor decodes exactly as before", () => {
  const parsed = parseBuzzCodingSessionMetadata(metadata());
  assert.equal(parsed?.agentRef, null);
  assert.equal(Object.hasOwn(parsed, "role"), false);
});

test("metadata carries the seat's role beside its actor", () => {
  const parsed = parseBuzzCodingSessionMetadata(
    metadata({ agentRef: ACTOR, role: "verifier" }),
  );
  assert.equal(parsed?.agentRef, ACTOR);
  assert.equal(parsed?.role, "verifier");
});

test("a role with no actor is malformed metadata, not a partial dialect", () => {
  assert.equal(
    parseBuzzCodingSessionMetadata(metadata({ role: "lead" })),
    null,
  );
  assert.equal(
    parseBuzzCodingSessionMetadata(metadata({ agentRef: ACTOR, role: "Lead" })),
    null,
  );
  assert.equal(
    parseBuzzCodingSessionMetadata(metadata({ agentRef: ACTOR, role: "" })),
    null,
  );
});

/**
 * A seat's role defaults to the agent's *home* role — what the pack behind it
 * actually knows — and every departure from it is said out loud, because the
 * pack that gets staged is still the home role's.
 */
test("an untouched role box fills from the agent's home role", () => {
  const agent = { pubkey: ACTOR, name: "Ada", homeRole: "builder" };
  assert.equal(
    defaultCodingSessionSeatRole({ agent, roleTouched: false, role: "" }),
    "builder",
  );
});

test("a role the person typed is never overwritten by a home role", () => {
  const agent = { pubkey: ACTOR, name: "Ada", homeRole: "builder" };
  assert.equal(
    defaultCodingSessionSeatRole({ agent, roleTouched: true, role: "lead" }),
    "lead",
  );
});

test("an agent with no home role fills nothing, and clearing the agent clears the role", () => {
  const bare = { pubkey: ACTOR, name: "Ada" };
  assert.equal(
    defaultCodingSessionSeatRole({
      agent: bare,
      roleTouched: false,
      role: "",
    }),
    "",
  );
  assert.equal(
    defaultCodingSessionSeatRole({
      agent: { pubkey: ACTOR, name: "Ada", homeRole: null },
      roleTouched: false,
      role: "",
    }),
    "",
  );
  assert.equal(
    defaultCodingSessionSeatRole({
      agent: null,
      roleTouched: true,
      role: "lead",
    }),
    "",
  );
});

test("a seat on its own home role says so, quietly", () => {
  const notice = codingSessionSeatRoleNotice({
    agent: { pubkey: ACTOR, name: "Ada", homeRole: "builder" },
    role: "builder",
  });
  assert.deepEqual(notice, { tone: "muted", message: "Its home role." });
});

test("a seat given someone else's role names the pack it will actually carry (LANE-L23: the seat's role, not the home role)", () => {
  const notice = codingSessionSeatRoleNotice({
    agent: { pubkey: ACTOR, name: "Ada", homeRole: "builder" },
    role: "lead",
  });
  assert.equal(notice?.tone, "warn");
  // Before LANE-L23's staging fix this asserted "it will carry the builder
  // pack" — the home role — which described a bug (the seat's role never
  // picked the pack) as if it were the product's actual behavior. The
  // staging rule now keys on the seat's role, so the disclosure does too.
  assert.equal(
    notice?.message,
    "Ada is a builder — seating it as lead; it will carry the lead pack.",
  );
});

test("an agent whose home role was never asked about claims nothing", () => {
  assert.equal(
    codingSessionSeatRoleNotice({
      agent: { pubkey: ACTOR, name: "Ada" },
      role: "lead",
    }),
    null,
  );
  assert.equal(
    codingSessionSeatRoleNotice({
      agent: { pubkey: ACTOR, name: "Ada", homeRole: null },
      role: "lead",
    }),
    null,
  );
  // No agent, or no role yet: there is no mismatch to disclose.
  assert.equal(
    codingSessionSeatRoleNotice({ agent: null, role: "lead" }),
    null,
  );
  assert.equal(
    codingSessionSeatRoleNotice({
      agent: { pubkey: ACTOR, name: "Ada", homeRole: "builder" },
      role: "  ",
    }),
    null,
  );
});
