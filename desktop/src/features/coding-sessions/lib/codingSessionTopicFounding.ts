/**
 * Founding a topic: the records that exist before anyone is asked to run.
 *
 * "New coding session" publishes only the founding facts — the channel
 * (minted here when a project has never had one) → the genesis (44226) → the
 * goal (44227, when one was given) → the name (44229, when one was given) —
 * and stops. No 44221, no provider, no agent process: Solo or Team, who
 * leads, the runtime, the bench and the policy are picked on the founded
 * session's page, and Start is what asks a provider to run it (Andy,
 * 2026-09-08, 2026-09-10). Since 2026-09-10 the click founds with a blank
 * goal and no name; both are published from the page as the founder commits
 * them.
 *
 * Three properties this module exists to hold:
 *
 * 1. **The genesis is the founding.** A genesis that published is a session,
 *    whatever happens after it; a genesis that did not is nothing, and the
 *    result says which.
 * 2. **A goal or name that failed after the genesis is reported, not
 *    swallowed.** The umbrella exists and the session screen can set both
 *    from its pills, so the result is `ok: true` with the publisher's own
 *    words attached — the same honesty `useCodingSessionCrewLaunch` keeps
 *    about the goal, and for the same reason: an empty pill implies nobody
 *    set one.
 * 3. **The order is a property of this function.** Every relay interaction is
 *    injected, so a test watches the real sequence rather than a re-enactment.
 *
 * `ensureProviderChannelMembership` is deliberately absent: there is no
 * provider yet, and Start runs it before the create the way the crew launch
 * does today.
 */
import type { CodingSessionCrewLaunchStep } from "./codingSessionCrewLaunch";

export const CODING_SESSION_FOUNDING_CHANNEL_STEP = "channel";
export const CODING_SESSION_FOUNDING_GENESIS_STEP = "genesis";
export const CODING_SESSION_FOUNDING_GOAL_STEP = "goal";
export const CODING_SESSION_FOUNDING_NAME_STEP = "name";

export type CodingSessionTopicFoundingInput = {
  /**
   * The channel the topic is founded in, or null when the caller has a
   * destination but no id for it yet — a project whose sessions channel is
   * published by the first act that needs one. Null requires
   * [`CodingSessionTopicFoundingDeps.ensureChannel`].
   */
  channelId: string | null;
  /** The goal to publish, or blank to publish none. */
  goal: string;
  /** The name to publish, or null / blank to publish none. */
  title: string | null;
  /** Kept on the result for the caller's own records; nothing here signs it. */
  projectRef: string | null;
  repoRef: string | null;
};

export type CodingSessionTopicFoundingDeps = {
  /** Resolve — creating it if needed — the channel. Called only when `channelId` is null. */
  ensureChannel?: () => Promise<string>;
  /** Mint the umbrella's session ref. */
  newSessionRef: () => string;
  publishGenesis: (input: {
    channelId: string;
    sessionRef: string;
  }) => Promise<{ eventId: string }>;
  publishGoal: (input: {
    channelId: string;
    content: string;
    sessionRef: string;
  }) => Promise<unknown>;
  publishName: (input: {
    channelId: string;
    content: string;
    sessionRef: string;
  }) => Promise<unknown>;
  /** Called after every step transition, for a UI that shows the sequence. */
  onSteps?: (steps: CodingSessionCrewLaunchStep[]) => void;
};

/** What became of one record that is not fatal to the founding. */
export type CodingSessionTopicFoundingOutcome = {
  /** True only when the record actually went out. */
  published: boolean;
  /** The publish's own words when it failed, else null. */
  reason: string | null;
};

export type CodingSessionTopicFoundingResult = {
  /** True exactly when the genesis is on the wire. */
  ok: boolean;
  /** The channel the topic was founded in; null only when none was settled. */
  channelId: string | null;
  /** Null only when the founding stopped before a genesis was published. */
  sessionRef: string | null;
  genesisRef: string | null;
  /**
   * `published: false` with `reason: null` means no goal was asked for —
   * distinct from a goal that was asked for and refused. Same for `name`.
   */
  goal: CodingSessionTopicFoundingOutcome;
  name: CodingSessionTopicFoundingOutcome;
  projectRef: string | null;
  repoRef: string | null;
  /** The step that stopped the founding, by id. Null on success. */
  failedStep: string | null;
  failureReason: string | null;
  steps: CodingSessionCrewLaunchStep[];
};

function step(id: string, label: string): CodingSessionCrewLaunchStep {
  return { id, label, state: "pending", detail: null };
}

/** The steps a founding will walk, before it walks any of them. */
export function planCodingSessionTopicFounding(
  input: Pick<CodingSessionTopicFoundingInput, "channelId" | "goal" | "title">,
): CodingSessionCrewLaunchStep[] {
  return [
    ...(input.channelId === null
      ? [
          step(
            CODING_SESSION_FOUNDING_CHANNEL_STEP,
            "Create the project's sessions channel",
          ),
        ]
      : []),
    step(CODING_SESSION_FOUNDING_GENESIS_STEP, "Found the session"),
    // A blank goal is not a step: a step that never runs would sit
    // "pending" in every `onSteps` emission for the rest of the founding.
    ...(hasText(input.goal)
      ? [step(CODING_SESSION_FOUNDING_GOAL_STEP, "Publish the goal")]
      : []),
    ...(hasTitle(input.title)
      ? [step(CODING_SESSION_FOUNDING_NAME_STEP, "Publish the name")]
      : []),
  ];
}

function hasTitle(title: string | null): title is string {
  return title !== null && hasText(title);
}

function hasText(text: string): boolean {
  return text.trim().length > 0;
}

function said(error: unknown, fallback: string): string {
  const message = (
    error instanceof Error ? error.message : String(error)
  ).trim();
  return message.length > 0 ? message : fallback;
}

/**
 * Found the topic: channel? → genesis → goal? → name?.
 *
 * Resolves, never rejects: a refusal is a result with `failedStep` set. The
 * genesis is the only fatal step; see the module comment.
 */
export async function foundCodingSessionTopic(
  input: CodingSessionTopicFoundingInput,
  deps: CodingSessionTopicFoundingDeps,
): Promise<CodingSessionTopicFoundingResult> {
  const steps = planCodingSessionTopicFounding(input);
  const emit = () => deps.onSteps?.(steps.map((entry) => ({ ...entry })));
  const mark = (
    id: string,
    state: CodingSessionCrewLaunchStep["state"],
    detail: string | null = null,
  ) => {
    const entry = steps.find((candidate) => candidate.id === id);
    if (entry) {
      entry.state = state;
      entry.detail = detail;
    }
    emit();
  };
  const result: CodingSessionTopicFoundingResult = {
    ok: false,
    channelId: input.channelId,
    sessionRef: null,
    genesisRef: null,
    goal: { published: false, reason: null },
    name: { published: false, reason: null },
    projectRef: input.projectRef,
    repoRef: input.repoRef,
    failedStep: null,
    failureReason: null,
    steps,
  };
  const fail = (id: string, reason: string) => {
    mark(id, "failed", reason);
    result.failedStep = id;
    result.failureReason = reason;
    return result;
  };
  emit();

  let channelId = input.channelId;
  if (channelId === null) {
    if (!deps.ensureChannel) {
      return fail(
        CODING_SESSION_FOUNDING_CHANNEL_STEP,
        "This topic has nowhere to be founded, and nothing here can create a channel for it.",
      );
    }
    mark(CODING_SESSION_FOUNDING_CHANNEL_STEP, "running");
    try {
      channelId = await deps.ensureChannel();
    } catch (error) {
      return fail(
        CODING_SESSION_FOUNDING_CHANNEL_STEP,
        said(error, "Could not prepare a channel for this project's sessions."),
      );
    }
    result.channelId = channelId;
    mark(CODING_SESSION_FOUNDING_CHANNEL_STEP, "done");
  }

  const sessionRef = deps.newSessionRef();
  mark(CODING_SESSION_FOUNDING_GENESIS_STEP, "running");
  try {
    const genesis = await deps.publishGenesis({ channelId, sessionRef });
    result.sessionRef = sessionRef;
    result.genesisRef = genesis.eventId;
  } catch (error) {
    return fail(
      CODING_SESSION_FOUNDING_GENESIS_STEP,
      said(error, "Failed to found the coding session."),
    );
  }
  mark(CODING_SESSION_FOUNDING_GENESIS_STEP, "done");
  // From here the session exists. Nothing below un-founds it, so nothing
  // below is fatal — but each is reported in the publisher's own words.
  result.ok = true;

  if (hasText(input.goal)) {
    mark(CODING_SESSION_FOUNDING_GOAL_STEP, "running");
    try {
      await deps.publishGoal({ channelId, content: input.goal, sessionRef });
      result.goal = { published: true, reason: null };
      mark(CODING_SESSION_FOUNDING_GOAL_STEP, "done");
    } catch (error) {
      const reason = said(error, "the goal publish did not go out");
      result.goal = { published: false, reason };
      mark(CODING_SESSION_FOUNDING_GOAL_STEP, "failed", reason);
    }
  }

  if (hasTitle(input.title)) {
    mark(CODING_SESSION_FOUNDING_NAME_STEP, "running");
    try {
      await deps.publishName({
        channelId,
        content: input.title.trim(),
        sessionRef,
      });
      result.name = { published: true, reason: null };
      mark(CODING_SESSION_FOUNDING_NAME_STEP, "done");
    } catch (error) {
      const reason = said(error, "the name publish did not go out");
      result.name = { published: false, reason };
      mark(CODING_SESSION_FOUNDING_NAME_STEP, "failed", reason);
    }
  }

  return result;
}
