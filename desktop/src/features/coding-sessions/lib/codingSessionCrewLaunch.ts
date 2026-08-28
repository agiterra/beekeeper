/**
 * Launching a team: one signed sequence, each step gated on the last one's
 * receipt.
 *
 * The channel the team lands in (minted here when a project has never had one)
 * → genesis → **one** create, for the lead → `grant-operator` to that seat →
 * its first turn, carrying the goal and the roster it may hire from.
 *
 * **D14: a launch seats the lead only.** The roster is not what the launch
 * creates; it is who the lead may bring in, one `bee sessions hire` at a time,
 * after it has heard the mission. That is a deliberate reversal of the earlier
 * design, and it buys back a limitation the person was previously made to
 * work around: a team launch created every seat against the one selected
 * runtime, so a Codex architect on a Claude provider disabled Launch outright
 * (item 79c). With one seat created here, each later seat picks its own
 * runtime at hire time.
 *
 * Three properties this module exists to hold:
 *
 * 1. **Nothing is published until the lead's model checks pass.** The check is
 *    decided on a (provider, model) pair this step has verified: the selected
 *    runtime must actually offer the seat's model, because a model it does not
 *    have is replaced by its default and the seat then runs on a vendor
 *    nothing checked. It is scoped to the seats this launch actually creates —
 *    refusing a launch over a seat it will never create would be refusing a
 *    fact that is not yet true.
 * 2. **A failed step names itself and leaves the earlier work alone.** A grant
 *    failing does not un-create the lead; it is a real execution, it is
 *    visible, and the report says exactly which step stopped.
 * 3. **The roster is disclosed as hireable, not as seated.** A first turn that
 *    listed three colleagues who do not exist would have the lead addressing
 *    empty seats for its whole first turn.
 *
 * Every relay interaction is injected, so the order above is a property of
 * this function rather than of the hook that calls it.
 */
import type { CodingSessionCommandTarget } from "./codingSessionCommand";
import {
  checkCodingSessionCrewFamilies,
  checkCodingSessionCrewSeatModels,
  codingSessionCrewRosterText,
  type ResolvedCodingSessionCrewSeat,
} from "./codingSessionCrew";

/** Where the launch is, one row per signed step. */
export type CodingSessionCrewLaunchStep = {
  id: string;
  label: string;
  state: "pending" | "running" | "done" | "failed";
  detail: string | null;
};

/** A seat the launch actually created, joined to the target its receipt minted. */
export type LaunchedCodingSessionCrewSeat = {
  seat: ResolvedCodingSessionCrewSeat;
  target: CodingSessionCommandTarget;
};

export type CodingSessionCrewLaunchResult = {
  ok: boolean;
  /**
   * The channel every event of this launch was published to.
   *
   * Not always the channel the caller passed in: a project whose sessions
   * channel did not exist yet passes `null`, and this is the id that was
   * minted for it. Null only when the launch stopped before a channel was
   * settled.
   */
  channelId: string | null;
  /** Null only when the refusal happened before the genesis was published. */
  sessionRef: string | null;
  genesisRef: string | null;
  /**
   * Seats whose create receipt landed, in creation order.
   *
   * Under D14 this is the lead alone on a successful launch — one entry, not
   * one per roster row.
   */
  seats: LaunchedCodingSessionCrewSeat[];
  /**
   * The rest of the roster: seats this launch deliberately did NOT create.
   *
   * Reported so a screen can say "shown, not launched" rather than letting a
   * roster of four imply four live agents. The lead hires these itself.
   */
  hireableSeats: ResolvedCodingSessionCrewSeat[];
  /**
   * Labels of the seats staged with no role pack on this computer, in seat
   * order.
   *
   * These seats run on their persona prompt alone: nothing wrote
   * `.agents/skills` into their working directory, so the craft their role
   * refers to is not on disk for them. Reported rather than hidden — a screen
   * that shows six seated roles and says nothing else implies six seats that
   * hold their roles' craft.
   */
  seatsWithoutRolePack: string[];
  /** The step that stopped the launch, by id. Null on success. */
  failedStep: string | null;
  failureReason: string | null;
  steps: CodingSessionCrewLaunchStep[];
};

export type CodingSessionCrewLaunchDeps = {
  /**
   * Resolve — creating it if needed — the channel this team launches into.
   *
   * Called once, and only when `input.channelId` is null, which is exactly the
   * project case: a project's sessions channel is published by the first
   * create that needs it, never on mount. The same helper the one-session path
   * calls, so a team launch and a single create bring the same channel into
   * existence and record the same project fact.
   */
  ensureChannel?: () => Promise<string>;
  /** Mint the umbrella's session ref. */
  newSessionRef: () => string;
  publishGenesis: (input: {
    channelId: string;
    sessionRef: string;
  }) => Promise<{ eventId: string }>;
  /**
   * Sign and publish one seat's 44221 create. Resolves with the command id the
   * receipt will carry — never with a target, because a published create is
   * not yet an execution.
   */
  publishSeatCreate: (input: {
    seat: ResolvedCodingSessionCrewSeat;
    index: number;
    /** The settled channel — never the caller's, which may have been null. */
    channelId: string;
    sessionRef: string;
    genesisRef: string;
  }) => Promise<{
    commandId: string;
    /**
     * Whether this seat's host-local custody entry carried a role pack.
     * `undefined` from a caller that does not stage seats at all.
     */
    packStaged?: boolean;
  }>;
  /**
   * Wait for the provider's 44224 receipt for that exact command. This is the
   * gate: the next seat is not published until this resolves.
   */
  awaitSeatReceipt: (input: {
    commandId: string;
    channelId: string;
    seat: ResolvedCodingSessionCrewSeat;
  }) => Promise<CodingSessionCommandTarget>;
  grantOperator: (input: {
    channelId: string;
    genesisRef: string;
    granteePubkey: string;
  }) => Promise<void>;
  sendFirstTurn: (input: {
    channelId: string;
    target: CodingSessionCommandTarget;
    text: string;
  }) => Promise<void>;
  /** Called after every step transition, for a UI that shows the sequence. */
  onSteps?: (steps: CodingSessionCrewLaunchStep[]) => void;
};

export type CodingSessionCrewLaunchInput = {
  /**
   * The channel every seat is created in, or null when the caller has a
   * destination but no id for it yet — a project whose sessions channel is
   * published by the first create that needs one. Null requires
   * [`CodingSessionCrewLaunchDeps.ensureChannel`].
   */
  channelId: string | null;
  goal: string;
  /** Seats in launch order, already resolved to actors. */
  seats: ReadonlyArray<ResolvedCodingSessionCrewSeat>;
  /** Persona id of the seat that receives the first turn. */
  primaryPersonaId: string;
  /**
   * The one provider runtime every seat will be created against.
   *
   * Required, and required to be non-empty: the vendor rule is decided on a
   * model, and a model the selected runtime cannot run is silently replaced by
   * that runtime's default. Without this the check would be reading a string
   * nothing verified.
   */
  provider: {
    allowedModels: readonly string[];
    /** `providerInstanceRef` every seat is created against. */
    instanceRef?: string | null;
    /** Runtime name, so a refusal can say which provider it means. */
    label?: string | null;
  };
};

/** Step ids, so a caller can talk about a failure without matching prose. */
export const CODING_SESSION_CREW_LAUNCH_CHANNEL_STEP = "channel";
export const CODING_SESSION_CREW_LAUNCH_GENESIS_STEP = "genesis";
export const CODING_SESSION_CREW_LAUNCH_GRANT_STEP = "grant-operator";
export const CODING_SESSION_CREW_LAUNCH_TURN_STEP = "first-turn";
export const CODING_SESSION_CREW_LAUNCH_FAMILY_STEP = "family-check";

/** The seat id of one create step. */
export function codingSessionCrewLaunchSeatStepId(index: number): string {
  return `create:${index}`;
}

/**
 * The steps a launch will walk, before it walks any of them.
 *
 * Rendered up front so a person sees the whole sequence — and so a launch that
 * dies on step two shows the four steps it never reached rather than an empty
 * list.
 */
export function planCodingSessionCrewLaunch(
  input: CodingSessionCrewLaunchInput,
): CodingSessionCrewLaunchStep[] {
  const lead = leadSeat(input.seats, input.primaryPersonaId);
  return [
    step(
      CODING_SESSION_CREW_LAUNCH_FAMILY_STEP,
      "Check the team's model families",
    ),
    // Only when there is nothing to launch into yet: a channel that already
    // exists is not a step anybody walks.
    ...(input.channelId === null
      ? [
          step(
            CODING_SESSION_CREW_LAUNCH_CHANNEL_STEP,
            "Create the project's sessions channel",
          ),
        ]
      : []),
    step(CODING_SESSION_CREW_LAUNCH_GENESIS_STEP, "Found the session"),
    // Exactly one, because exactly one seat is created (D14). A step list
    // showing four creates would be the same lie the roster used to tell.
    ...(lead
      ? [
          step(
            codingSessionCrewLaunchSeatStepId(0),
            `Seat ${lead.actorLabel} as ${lead.role}`,
          ),
        ]
      : []),
    step(
      CODING_SESSION_CREW_LAUNCH_GRANT_STEP,
      `Grant the ${lead?.role ?? "lead"} seat operator authority`,
    ),
    step(
      CODING_SESSION_CREW_LAUNCH_TURN_STEP,
      "Send the goal and the team it may hire",
    ),
  ];
}

/**
 * The seat that holds operator authority for the crew: the `lead`, or — when
 * the crew has no lead — the primary, which is the seat the first turn goes
 * to and therefore the only other defensible choice.
 */
export function leadSeat(
  seats: ReadonlyArray<ResolvedCodingSessionCrewSeat>,
  primaryPersonaId: string,
): ResolvedCodingSessionCrewSeat | null {
  return (
    seats.find((seat) => seat.role === "lead") ??
    seats.find((seat) => seat.personaId === primaryPersonaId) ??
    null
  );
}

/**
 * Split the roster into the seat this launch creates and the seats it does
 * not.
 *
 * One place, so the plan, the launch and the screen cannot disagree about
 * which rows are live and which are an offer.
 */
export function partitionCodingSessionCrewLaunchSeats(
  seats: ReadonlyArray<ResolvedCodingSessionCrewSeat>,
  primaryPersonaId: string,
): {
  lead: ResolvedCodingSessionCrewSeat | null;
  hireable: ResolvedCodingSessionCrewSeat[];
} {
  const lead = leadSeat(seats, primaryPersonaId);
  return {
    lead,
    hireable: lead
      ? seats.filter((seat) => seat.personaId !== lead.personaId)
      : [...seats],
  };
}

/**
 * The lead's first turn: the goal, then the team it may hire — labelled as an
 * offer, with the verb that takes it up.
 *
 * The distinction is the whole point. A roster that reads like a seated team
 * has the lead addressing three agents that do not exist, waiting for reports
 * that cannot come; naming it as hireable, with the command, is what turns the
 * same list into work it can actually start.
 */
export function codingSessionCrewLeadFirstTurnText(input: {
  goal: string;
  lead: ResolvedCodingSessionCrewSeat;
  hireable: ReadonlyArray<ResolvedCodingSessionCrewSeat>;
}): string {
  const goal = input.goal.trim();
  if (input.hireable.length === 0) {
    return `${goal}\n\nYou are seated. Nobody else is — this team has no other roles on this computer.`;
  }
  const roster = codingSessionCrewRosterText({
    seats: input.hireable,
    primaryPersonaId: input.lead.personaId,
  });
  return [
    goal,
    "",
    "You are seated. Nobody else is: the roster below is who you may hire, " +
      "one at a time, once you know what the work is.",
    "",
    roster,
    "",
    "Hire with:",
    "  bee sessions hire --channel <uuid> --session-ref <uuid> --role <role> --brief <file>",
    "A hired seat's first turn IS the brief — do not send a second start " +
      "message. End your turn after hiring.",
  ].join("\n");
}

export async function launchCodingSessionCrew(
  input: CodingSessionCrewLaunchInput,
  deps: CodingSessionCrewLaunchDeps,
): Promise<CodingSessionCrewLaunchResult> {
  const steps = planCodingSessionCrewLaunch(input);
  const emit = () => deps.onSteps?.(steps.map((entry) => ({ ...entry })));
  const mark = (
    id: string,
    state: CodingSessionCrewLaunchStep["state"],
    detail: string | null = null,
  ) => {
    const found = steps.find((entry) => entry.id === id);
    if (found) {
      found.state = state;
      found.detail = detail;
    }
    emit();
  };
  const seated: LaunchedCodingSessionCrewSeat[] = [];
  // Filled in once the lead is known; a refusal before that reports an empty
  // roster rather than guessing which seat would have been created.
  let hireable: ResolvedCodingSessionCrewSeat[] = [];
  const seatsWithoutRolePack: string[] = [];
  // Settled below, before the genesis. Everything published by this launch
  // goes here, so a project that had no channel gets one and then gets its
  // whole team in it.
  let channelId: string | null = input.channelId;
  const fail = (
    id: string,
    reason: string,
    sessionRef: string | null,
    genesisRef: string | null,
  ): CodingSessionCrewLaunchResult => {
    mark(id, "failed", reason);
    return {
      ok: false,
      channelId,
      sessionRef,
      genesisRef,
      seats: seated,
      hireableSeats: [...hireable],
      seatsWithoutRolePack,
      failedStep: id,
      failureReason: reason,
      steps: steps.map((entry) => ({ ...entry })),
    };
  };

  emit();

  // Before anything is signed. A refusal here must cost nothing.
  mark(CODING_SESSION_CREW_LAUNCH_FAMILY_STEP, "running");
  const primary = input.seats.find(
    (seat) => seat.personaId === input.primaryPersonaId,
  );
  if (input.seats.length === 0 || !primary) {
    return fail(
      CODING_SESSION_CREW_LAUNCH_FAMILY_STEP,
      "This team has no seat to address its first turn to.",
      null,
      null,
    );
  }
  const lead = leadSeat(input.seats, input.primaryPersonaId);
  if (!lead) {
    return fail(
      CODING_SESSION_CREW_LAUNCH_FAMILY_STEP,
      "This team has no lead seat to hold operator authority.",
      null,
      null,
    );
  }
  const { hireable: rest } = partitionCodingSessionCrewLaunchSeats(
    input.seats,
    input.primaryPersonaId,
  );
  hireable = rest;
  // Scoped to the seats this launch actually creates — today, the lead alone.
  // Checking the whole roster here refused launches over a seat that was never
  // going to be created against this runtime (item 79c); each hired seat's own
  // runtime is decided, and checked, when the lead hires it.
  //
  // Before the vendor rule, because the vendor rule reads the model: a model
  // this provider cannot run is a model the seat will not run on.
  const created = [lead];
  const runnable = checkCodingSessionCrewSeatModels(created, input.provider);
  if (!runnable.ok) {
    return fail(
      CODING_SESSION_CREW_LAUNCH_FAMILY_STEP,
      runnable.reason,
      null,
      null,
    );
  }
  const family = checkCodingSessionCrewFamilies(created);
  if (!family.ok) {
    return fail(
      CODING_SESSION_CREW_LAUNCH_FAMILY_STEP,
      family.reason,
      null,
      null,
    );
  }
  mark(CODING_SESSION_CREW_LAUNCH_FAMILY_STEP, "done");

  if (channelId === null) {
    mark(CODING_SESSION_CREW_LAUNCH_CHANNEL_STEP, "running");
    if (!deps.ensureChannel) {
      return fail(
        CODING_SESSION_CREW_LAUNCH_CHANNEL_STEP,
        "This team has no channel to launch into, and nothing here can create one.",
        null,
        null,
      );
    }
    try {
      channelId = await deps.ensureChannel();
    } catch (error) {
      return fail(
        CODING_SESSION_CREW_LAUNCH_CHANNEL_STEP,
        describe(
          error,
          "The channel for this project's sessions could not be created.",
        ),
        null,
        null,
      );
    }
    mark(CODING_SESSION_CREW_LAUNCH_CHANNEL_STEP, "done");
  }
  const launchChannelId = channelId;

  mark(CODING_SESSION_CREW_LAUNCH_GENESIS_STEP, "running");
  const sessionRef = deps.newSessionRef();
  let genesisRef: string;
  try {
    genesisRef = (
      await deps.publishGenesis({ channelId: launchChannelId, sessionRef })
    ).eventId;
  } catch (error) {
    return fail(
      CODING_SESSION_CREW_LAUNCH_GENESIS_STEP,
      describe(error, "The session could not be founded."),
      sessionRef,
      null,
    );
  }
  mark(CODING_SESSION_CREW_LAUNCH_GENESIS_STEP, "done");

  for (const [index, seat] of created.entries()) {
    const id = codingSessionCrewLaunchSeatStepId(index);
    mark(id, "running");
    try {
      const { commandId, packStaged } = await deps.publishSeatCreate({
        seat,
        index,
        channelId: launchChannelId,
        sessionRef,
        genesisRef,
      });
      if (packStaged === false) seatsWithoutRolePack.push(seat.actorLabel);
      // The gate. The next seat's create is not signed until this receipt
      // lands, so a provider that refuses seat two never sees seat three.
      const target = await deps.awaitSeatReceipt({
        commandId,
        channelId: launchChannelId,
        seat,
      });
      seated.push({ seat, target });
    } catch (error) {
      return fail(
        id,
        describe(
          error,
          `The ${seat.role} seat was not created. The seats before it are live.`,
        ),
        sessionRef,
        genesisRef,
      );
    }
    mark(
      id,
      "done",
      seatsWithoutRolePack.includes(seat.actorLabel)
        ? "seated with no role skills — this computer has no role pack behind this persona"
        : null,
    );
  }

  mark(CODING_SESSION_CREW_LAUNCH_GRANT_STEP, "running");
  try {
    await deps.grantOperator({
      channelId: launchChannelId,
      genesisRef,
      granteePubkey: lead.actor,
    });
  } catch (error) {
    return fail(
      CODING_SESSION_CREW_LAUNCH_GRANT_STEP,
      describe(
        error,
        `${lead.actorLabel} was seated but could not be granted operator, so it cannot steer its siblings.`,
      ),
      sessionRef,
      genesisRef,
    );
  }
  mark(CODING_SESSION_CREW_LAUNCH_GRANT_STEP, "done");

  mark(CODING_SESSION_CREW_LAUNCH_TURN_STEP, "running");
  const leadTarget = seated.find(
    (entry) => entry.seat.personaId === lead.personaId,
  )?.target;
  if (!leadTarget) {
    return fail(
      CODING_SESSION_CREW_LAUNCH_TURN_STEP,
      "The lead seat has no execution to send the goal to.",
      sessionRef,
      genesisRef,
    );
  }
  try {
    await deps.sendFirstTurn({
      channelId: launchChannelId,
      target: leadTarget,
      text: codingSessionCrewLeadFirstTurnText({
        goal: input.goal,
        lead,
        hireable,
      }),
    });
  } catch (error) {
    return fail(
      CODING_SESSION_CREW_LAUNCH_TURN_STEP,
      describe(
        error,
        "The team is seated but the goal was not delivered. Send it from the session composer.",
      ),
      sessionRef,
      genesisRef,
    );
  }
  mark(CODING_SESSION_CREW_LAUNCH_TURN_STEP, "done");

  return {
    ok: true,
    channelId: launchChannelId,
    sessionRef,
    genesisRef,
    seats: seated,
    hireableSeats: hireable,
    seatsWithoutRolePack,
    failedStep: null,
    failureReason: null,
    steps: steps.map((entry) => ({ ...entry })),
  };
}

function step(id: string, label: string): CodingSessionCrewLaunchStep {
  return { id, label, state: "pending", detail: null };
}

function describe(error: unknown, fallback: string): string {
  const message = error instanceof Error ? error.message.trim() : "";
  return message.length > 0 ? `${fallback} ${message}` : fallback;
}
