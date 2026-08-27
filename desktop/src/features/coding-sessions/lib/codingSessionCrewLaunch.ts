/**
 * Launching a crew: one signed sequence, each step gated on the last one's
 * receipt.
 *
 * Genesis → a create per seat, in the crew's seat order → `grant-operator` to
 * the lead seat → the first turn to the primary, carrying the goal and the
 * roster (plan D8).
 *
 * Two properties this module exists to hold:
 *
 * 1. **Nothing is published until the family check passes.** The verifier /
 *    builder vendor rule is a refusal, not a warning, and it runs before the
 *    genesis — a crew that fails it leaves no events behind at all.
 * 2. **A failed step names itself and leaves the earlier seats alone.** Seat
 *    three failing does not un-create seats one and two; they are real, they
 *    are visible, and the report says exactly which step stopped. Rolling
 *    them back would delete work the provider may already be doing.
 *
 * Every relay interaction is injected, so the order above is a property of
 * this function rather than of the hook that calls it.
 */
import type { CodingSessionCommandTarget } from "./codingSessionCommand";
import {
  checkCodingSessionCrewFamilies,
  codingSessionCrewFirstTurnText,
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
  /** Null only when the refusal happened before the genesis was published. */
  sessionRef: string | null;
  genesisRef: string | null;
  /** Seats whose create receipt landed, in creation order. */
  seats: LaunchedCodingSessionCrewSeat[];
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
    seat: ResolvedCodingSessionCrewSeat;
  }) => Promise<CodingSessionCommandTarget>;
  grantOperator: (input: {
    genesisRef: string;
    granteePubkey: string;
  }) => Promise<void>;
  sendFirstTurn: (input: {
    target: CodingSessionCommandTarget;
    text: string;
  }) => Promise<void>;
  /** Called after every step transition, for a UI that shows the sequence. */
  onSteps?: (steps: CodingSessionCrewLaunchStep[]) => void;
};

export type CodingSessionCrewLaunchInput = {
  channelId: string;
  goal: string;
  /** Seats in launch order, already resolved to actors. */
  seats: ReadonlyArray<ResolvedCodingSessionCrewSeat>;
  /** Persona id of the seat that receives the first turn. */
  primaryPersonaId: string;
};

/** Step ids, so a caller can talk about a failure without matching prose. */
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
  return [
    step(
      CODING_SESSION_CREW_LAUNCH_FAMILY_STEP,
      "Check the crew's model families",
    ),
    step(CODING_SESSION_CREW_LAUNCH_GENESIS_STEP, "Found the session"),
    ...input.seats.map((seat, index) =>
      step(
        codingSessionCrewLaunchSeatStepId(index),
        `Seat ${seat.actorLabel} as ${seat.role}`,
      ),
    ),
    step(
      CODING_SESSION_CREW_LAUNCH_GRANT_STEP,
      `Grant the ${leadSeat(input.seats, input.primaryPersonaId)?.role ?? "lead"} seat operator authority`,
    ),
    step(CODING_SESSION_CREW_LAUNCH_TURN_STEP, "Send the goal and the roster"),
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
  const seatsWithoutRolePack: string[] = [];
  const fail = (
    id: string,
    reason: string,
    sessionRef: string | null,
    genesisRef: string | null,
  ): CodingSessionCrewLaunchResult => {
    mark(id, "failed", reason);
    return {
      ok: false,
      sessionRef,
      genesisRef,
      seats: seated,
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
      "This crew has no seat to address its first turn to.",
      null,
      null,
    );
  }
  const family = checkCodingSessionCrewFamilies(input.seats);
  if (!family.ok) {
    return fail(
      CODING_SESSION_CREW_LAUNCH_FAMILY_STEP,
      family.reason,
      null,
      null,
    );
  }
  const lead = leadSeat(input.seats, input.primaryPersonaId);
  if (!lead) {
    return fail(
      CODING_SESSION_CREW_LAUNCH_FAMILY_STEP,
      "This crew has no lead seat to hold operator authority.",
      null,
      null,
    );
  }
  mark(CODING_SESSION_CREW_LAUNCH_FAMILY_STEP, "done");

  mark(CODING_SESSION_CREW_LAUNCH_GENESIS_STEP, "running");
  const sessionRef = deps.newSessionRef();
  let genesisRef: string;
  try {
    genesisRef = (
      await deps.publishGenesis({ channelId: input.channelId, sessionRef })
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

  for (const [index, seat] of input.seats.entries()) {
    const id = codingSessionCrewLaunchSeatStepId(index);
    mark(id, "running");
    try {
      const { commandId, packStaged } = await deps.publishSeatCreate({
        seat,
        index,
        sessionRef,
        genesisRef,
      });
      if (packStaged === false) seatsWithoutRolePack.push(seat.actorLabel);
      // The gate. The next seat's create is not signed until this receipt
      // lands, so a provider that refuses seat two never sees seat three.
      const target = await deps.awaitSeatReceipt({ commandId, seat });
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
    await deps.grantOperator({ genesisRef, granteePubkey: lead.actor });
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
  const primaryTarget = seated.find(
    (entry) => entry.seat.personaId === input.primaryPersonaId,
  )?.target;
  if (!primaryTarget) {
    return fail(
      CODING_SESSION_CREW_LAUNCH_TURN_STEP,
      "The primary seat has no execution to send the goal to.",
      sessionRef,
      genesisRef,
    );
  }
  try {
    await deps.sendFirstTurn({
      target: primaryTarget,
      text: codingSessionCrewFirstTurnText({
        goal: input.goal,
        seats: input.seats,
        primaryPersonaId: input.primaryPersonaId,
      }),
    });
  } catch (error) {
    return fail(
      CODING_SESSION_CREW_LAUNCH_TURN_STEP,
      describe(
        error,
        "The crew is seated but the goal was not delivered. Send it from the session composer.",
      ),
      sessionRef,
      genesisRef,
    );
  }
  mark(CODING_SESSION_CREW_LAUNCH_TURN_STEP, "done");

  return {
    ok: true,
    sessionRef,
    genesisRef,
    seats: seated,
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
