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
  codingSessionSeatStagedFromLine,
  type PackRef,
} from "./codingSessionPackRef";
import { codingSessionProjectActionsGrantDisclosure } from "./codingSessionProjectActionsGrant";
import { buildCodingSessionTranscriptGenerationId } from "./codingSessionTranscriptPresentation";
import {
  checkCodingSessionCrewFamilies,
  checkCodingSessionCrewSeatModels,
  type ResolvedCodingSessionCrewSeat,
} from "./codingSessionCrew";
import {
  type CodingSessionCrewHireRoster,
  codingSessionCrewLeadFirstTurnText,
} from "./codingSessionCrewLaunchFirstTurn";

export {
  type CodingSessionCrewHireRoster,
  type CodingSessionCrewHireRosterAgent,
  codingSessionCrewLeadFirstTurnText,
} from "./codingSessionCrewLaunchFirstTurn";

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
  /**
   * The directory the lead's create was staged against on this computer.
   *
   * The worktree this launch made when it was asked for one, else the
   * checkout it was given, else null. Host-local — it is never on the wire —
   * but it is what a screen has to name to be honest about where the lead is
   * actually working.
   */
  leadWorkdir: string | null;
  /**
   * The kind:44245 this launch published, or null when it published none.
   *
   * Null is two different facts — no policy was written, or the launch stopped
   * before the policy step — and the step list is what separates them.
   */
  policyEventId: string | null;
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
  /**
   * Create the lead's own git worktree, host-locally, before anything is
   * signed. Resolves with the directory it actually landed in — which may
   * carry a disambiguating suffix the form never showed.
   *
   * Absent from a caller that cannot make worktrees; asking for one anyway is
   * then a named refusal rather than a silent fall back into the checkout.
   */
  createLeadWorktree?: (input: {
    workdir: string;
    name: string;
    source: string | null;
  }) => Promise<{ path: string }>;
  publishSeatCreate: (input: {
    seat: ResolvedCodingSessionCrewSeat;
    index: number;
    /** The settled channel — never the caller's, which may have been null. */
    channelId: string;
    sessionRef: string;
    genesisRef: string;
    /**
     * The project coordinate this session belongs to, signed into the create.
     *
     * Null is a real answer — a launch with no project — and it is the answer
     * a screen has to disclose rather than let the session land nowhere the
     * person was looking (item 87).
     */
    projectRef: string | null;
    /**
     * The repository coordinate signed into this seat's create, or null.
     * Same rule as `projectRef` — a real answer, disclosed rather than
     * guessed (LANE-L20, finding 38).
     */
    repoRef: string | null;
    /**
     * Where the lead runs on this computer: the worktree this launch created,
     * else the checkout it was given, else null. Never published.
     */
    workdir: string | null;
  }) => Promise<{
    commandId: string;
    /**
     * Whether this seat's host-local custody entry carried a role pack.
     * `undefined` from a caller that does not stage seats at all.
     */
    packStaged?: boolean;
    /**
     * The repository commit that pack was staged from, when one vouched for
     * it (finding 84). `null` or `undefined` prints no provenance.
     */
    packRef?: PackRef | null;
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
  grantLeadSeat: (input: {
    channelId: string;
    genesisRef: string;
    actorPubkey: string;
    role: string;
  }) => Promise<void>;
  /**
   * Delegate this project's action definitions and manual runs to the lead,
   * when the launch names a project (ledger 186, finding 178(f) —
   * `codingSessionProjectActionsGrant.ts` says what the delegation is).
   * Absent from a caller that cannot sign one. Resolves with the sentence to
   * disclose on the grant step, or null when the lead holds it.
   */
  grantProjectActions?: (input: {
    channelId: string;
    genesisRef: string;
    actorPubkey: string;
    projectRef: string;
    leadLabel: string;
  }) => Promise<string | null>;
  /**
   * Publish the session's kind:44245 policy, when the form set one.
   *
   * Between the genesis and the first create on purpose: the policy is a fact
   * about the umbrella, and a seat that reads its own session should be able
   * to find the posture and the budget it is working under already on the
   * wire rather than in the founder's memory. Absent from a caller that
   * cannot publish one; a launch that was asked for a policy and has no way
   * to publish it is a named refusal rather than a silent omission.
   */
  publishPolicy?: (input: {
    channelId: string;
    sessionRef: string;
    genesisRef: string;
  }) => Promise<{ eventId: string }>;
  /**
   * Run the policy draft through the native validator **before** anything is
   * signed, and reject with core's own sentence when it refuses.
   *
   * Every 44245 rule lives in `buzz-core`, which is right, and the consequence
   * was that a cross-field refusal — `tokensPerSeat` above `tokensPerSession`,
   * say — was first evaluated after the genesis and the goal were already on
   * the wire (REVIEW-B3 F4). The dry run costs one call and moves the refusal
   * back to where it costs nothing.
   */
  validatePolicy?: () => Promise<void>;
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
   * The project this session belongs to, or null when it belongs to none.
   *
   * Signed into the lead's create exactly as the one-session path signs it, so
   * a team launched from inside a project lands in that project's session list
   * instead of in a channel nobody was looking at (item 87).
   */
  projectRef?: string | null;
  /**
   * The repository coordinate (`30617:<owner>:<d>`) this session's creates
   * should name, or null when none is known.
   *
   * LANE-L20 (finding 38): signed into the lead's create exactly as
   * `projectRef` is — every seat this launch creates carried `repoRef: null`
   * unconditionally before this, so the push path's own rule
   * (`useCodingSessionMissionLand.ts`) could never resolve a repository for a
   * team-launched session no matter how clearly the checkout named one.
   */
  repoRef?: string | null;
  /**
   * The checkout the lead is launched against on this computer, or null when
   * the person named none. Host-local; never on the wire.
   */
  workdir?: string | null;
  /**
   * Give the lead a worktree of its own, cut from `workdir` before the create
   * is signed.
   *
   * Null runs the lead directly in `workdir` — which is what shipped, and what
   * put a lead in the operator's own hot checkout while every seat it hired
   * got a worktree (item 87d).
   */
  leadWorktree?: { name: string; source: string | null } | null;
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
  /**
   * True when the launch form set at least one policy field.
   *
   * A launch that sets none publishes none: the withdrawal record is a
   * deliberate act of taking a policy back, not the default shape of a
   * session nobody wrote a policy for.
   */
  policySet?: boolean;
  /**
   * Start an umbrella that is already founded — a genesis with a goal and a
   * name, and nothing running — instead of founding one.
   *
   * Set, the launch publishes no channel and no genesis: it seats the lead
   * under these refs, publishes the policy, grants, and sends the first turn.
   * `channelId` is then required — the umbrella already lives in a channel,
   * so a launch given none to seat the lead in is refused at the step that
   * costs nothing, never handed a second channel.
   */
  existingUmbrella?: { sessionRef: string; genesisRef: string } | null;
  /**
   * The agents on this computer the lead may hire for this session, stated in
   * its first turn. Absent, the first turn offers the launch's other seats.
   */
  hireRoster?: CodingSessionCrewHireRoster | null;
};

/** Step ids, so a caller can talk about a failure without matching prose. */
export const CODING_SESSION_CREW_LAUNCH_WORKTREE_STEP = "lead-worktree";
export const CODING_SESSION_CREW_LAUNCH_CHANNEL_STEP = "channel";
export const CODING_SESSION_CREW_LAUNCH_GENESIS_STEP = "genesis";
export const CODING_SESSION_CREW_LAUNCH_POLICY_STEP = "policy";
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
    // Host-local and before anything is signed, so a worktree that cannot be
    // cut costs the launch nothing but a named refusal.
    ...(leadWorktreeRequest(input)
      ? [
          step(
            CODING_SESSION_CREW_LAUNCH_WORKTREE_STEP,
            `Create the ${lead?.role ?? "lead"} seat's worktree`,
          ),
        ]
      : []),
    // Only when there is nothing to launch into yet: a channel that already
    // exists is not a step anybody walks — and a founded umbrella already has
    // one, so a launch given none is refused rather than handed a second.
    ...(input.channelId === null && !input.existingUmbrella
      ? [
          step(
            CODING_SESSION_CREW_LAUNCH_CHANNEL_STEP,
            "Create the project's sessions channel",
          ),
        ]
      : []),
    // A founded umbrella is not founded twice: its genesis is a record that
    // exists, and a step for it would describe publishing one that is not.
    ...(input.existingUmbrella
      ? []
      : [step(CODING_SESSION_CREW_LAUNCH_GENESIS_STEP, "Found the session")]),
    // Only when one was written. A step that always appears and usually does
    // nothing teaches a reader to stop reading the list.
    ...(input.policySet
      ? [
          step(
            CODING_SESSION_CREW_LAUNCH_POLICY_STEP,
            "Publish the session policy",
          ),
        ]
      : []),
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
 * The worktree this launch will cut for the lead, or null when it will not.
 *
 * One place, so the step list and the launch cannot disagree about whether a
 * worktree is part of the sequence — a plan that promises one and a launch
 * that quietly skips it is the same class of lie as the roster that listed
 * four seats and created one.
 */
export function leadWorktreeRequest(
  input: Pick<CodingSessionCrewLaunchInput, "leadWorktree" | "workdir">,
): {
  workdir: string;
  name: string;
  source: string | null;
} | null {
  const workdir = input.workdir?.trim() ?? "";
  const name = input.leadWorktree?.name.trim() ?? "";
  if (workdir.length === 0 || name.length === 0) return null;
  return {
    workdir,
    name,
    source: input.leadWorktree?.source ?? null,
  };
}

/**
 * What the tab says about where this session will live, before it is launched.
 *
 * A team launched from inside a project used to sign `projectRef: null` and
 * then close the dialog, so the session existed and the person could not find
 * it (item 87). Naming the project — or saying plainly that there is none — is
 * the difference between a destination and a guess.
 */
export function codingSessionCrewProjectNote(
  projectName: string | null,
): string {
  const name = projectName?.trim() ?? "";
  return name.length > 0
    ? `Launching in ${name}: the session belongs to that project and appears in its sessions.`
    : "This session will not belong to a project — it lives in the channel above, not in a project's sessions.";
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
 * The session the person should be looking at after a successful launch.
 *
 * The one-session path navigates on its create's receipt; a team launch closed
 * the dialog and navigated nowhere, so a session that really had been founded
 * appeared nowhere the person had been looking (item 87b). The lead's receipt
 * carries the exact target, and the generation id is a pure function of
 * (channel, provider authority, target) — the same one the catalog mints — so
 * this needs no catalog round-trip and cannot open a different session.
 *
 * Null when there is nothing honest to open: a failed launch, a launch with no
 * settled channel, or a provider authority the caller could not name.
 */
export function codingSessionCrewLeadDestination(input: {
  result: Pick<
    CodingSessionCrewLaunchResult,
    "ok" | "channelId" | "seats" | "hireableSeats"
  >;
  providerAuthorityPubkey: string | null;
}): { channelId: string; generationId: string } | null {
  const { result, providerAuthorityPubkey } = input;
  if (!result.ok || !result.channelId || !providerAuthorityPubkey) return null;
  const lead = result.seats[0];
  if (!lead) return null;
  return {
    channelId: result.channelId,
    generationId: buildCodingSessionTranscriptGenerationId(
      result.channelId,
      providerAuthorityPubkey,
      lead.target,
    ),
  };
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
  // Where the lead will actually run. Starts as the checkout the person named
  // and is replaced by the worktree step when one is asked for.
  let leadWorkdir: string | null = input.workdir?.trim() || null;
  // The signed policy this launch published, or null when it published none.
  let policyEventId: string | null = null;
  const fail = (
    id: string,
    reason: string,
    sessionRef: string | null,
    genesisRef: string | null,
  ): CodingSessionCrewLaunchResult => {
    // A failure after the genesis leaves a real umbrella on the relay with no
    // seats and no grants. The step list says so only as an icon and a colour;
    // this says it in words, and names the ids, because a session the person
    // cannot find is the same defect as a badge pointing at a message that is
    // not there (REVIEW-B3 F4).
    const orphaned =
      genesisRef !== null && seated.length === 0
        ? ` This session was already founded and has no seats: session ${sessionRef}, genesis ${genesisRef}. Nothing else was published.`
        : "";
    const said = `${reason}${orphaned}`;
    mark(id, "failed", said);
    return {
      ok: false,
      channelId,
      sessionRef,
      genesisRef,
      seats: seated,
      hireableSeats: [...hireable],
      seatsWithoutRolePack,
      leadWorkdir,
      policyEventId,
      failedStep: id,
      failureReason: said,
      steps: steps.map((entry) => ({ ...entry })),
    };
  };

  emit();

  // Before anything is signed. A refusal here must cost nothing.
  mark(CODING_SESSION_CREW_LAUNCH_FAMILY_STEP, "running");
  const existing = input.existingUmbrella ?? null;
  if (existing && channelId === null) {
    // The umbrella was founded in a channel; a launch that cannot name it
    // would either mint a second channel or seat the lead nowhere. Refused
    // here, where nothing has been signed, and the result still names the
    // umbrella it was asked to start.
    return fail(
      CODING_SESSION_CREW_LAUNCH_FAMILY_STEP,
      "This session is already founded in a channel, and the launch was given none to seat the lead in.",
      existing.sessionRef,
      existing.genesisRef,
    );
  }
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
  // Still inside the step that costs nothing: the policy is validated by the
  // one implementation of its rules before the session is founded, so a
  // refusal never leaves a founded, seatless umbrella behind.
  if (input.policySet && deps.validatePolicy) {
    try {
      await deps.validatePolicy();
    } catch (error) {
      return fail(
        CODING_SESSION_CREW_LAUNCH_FAMILY_STEP,
        describe(error, "The session policy was refused."),
        null,
        null,
      );
    }
  }
  mark(CODING_SESSION_CREW_LAUNCH_FAMILY_STEP, "done");

  // The lead's own tree, cut before anything is signed. The create's workdir
  // *is* this path, so making it afterwards would publish a create pointing at
  // a directory that does not exist yet — the same order the one-session path
  // uses.
  const worktree = leadWorktreeRequest(input);
  if (worktree) {
    mark(CODING_SESSION_CREW_LAUNCH_WORKTREE_STEP, "running");
    if (!deps.createLeadWorktree) {
      return fail(
        CODING_SESSION_CREW_LAUNCH_WORKTREE_STEP,
        "This launch asked for a worktree for the lead, and nothing here can create one.",
        null,
        null,
      );
    }
    try {
      leadWorkdir = (await deps.createLeadWorktree(worktree)).path;
    } catch (error) {
      return fail(
        CODING_SESSION_CREW_LAUNCH_WORKTREE_STEP,
        describe(error, "The lead's worktree could not be created."),
        null,
        null,
      );
    }
    mark(CODING_SESSION_CREW_LAUNCH_WORKTREE_STEP, "done", leadWorkdir);
  }

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

  // The umbrella's refs: minted and founded here, or — for a session founded
  // before this launch — the ones it already has. `newSessionRef` and
  // `publishGenesis` are never called for an existing umbrella: a second
  // genesis under the same ref is the disputed case the founded projection
  // refuses to show.
  let sessionRef: string;
  let genesisRef: string;
  if (existing) {
    sessionRef = existing.sessionRef;
    genesisRef = existing.genesisRef;
  } else {
    mark(CODING_SESSION_CREW_LAUNCH_GENESIS_STEP, "running");
    sessionRef = deps.newSessionRef();
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
  }

  if (input.policySet) {
    mark(CODING_SESSION_CREW_LAUNCH_POLICY_STEP, "running");
    if (!deps.publishPolicy) {
      return fail(
        CODING_SESSION_CREW_LAUNCH_POLICY_STEP,
        "This launch set a policy, and nothing here can publish one.",
        sessionRef,
        genesisRef,
      );
    }
    try {
      // Fatal, unlike the goal: a goal that fails to publish still reaches the
      // lead in its first turn, but a policy that fails to publish is a
      // ceiling the founder set and nobody can read. Launching anyway would
      // put a team to work under limits that exist only on the screen that is
      // about to close.
      policyEventId = (
        await deps.publishPolicy({
          channelId: launchChannelId,
          sessionRef,
          genesisRef,
        })
      ).eventId;
    } catch (error) {
      return fail(
        CODING_SESSION_CREW_LAUNCH_POLICY_STEP,
        describe(error, "The session policy was not published."),
        sessionRef,
        genesisRef,
      );
    }
    mark(CODING_SESSION_CREW_LAUNCH_POLICY_STEP, "done");
  }

  for (const [index, seat] of created.entries()) {
    const id = codingSessionCrewLaunchSeatStepId(index);
    mark(id, "running");
    // Said on the step once the seat is confirmed: which repository commit
    // the pack came from, when one vouched for it (finding 84).
    let stagedFrom: string | null = null;
    try {
      const { commandId, packStaged, packRef } = await deps.publishSeatCreate({
        seat,
        index,
        channelId: launchChannelId,
        sessionRef,
        genesisRef,
        projectRef: input.projectRef ?? null,
        repoRef: input.repoRef ?? null,
        workdir: leadWorkdir,
      });
      if (packStaged === false) seatsWithoutRolePack.push(seat.actorLabel);
      stagedFrom = codingSessionSeatStagedFromLine(packRef ?? null);
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
        : stagedFrom,
    );
  }

  mark(CODING_SESSION_CREW_LAUNCH_GRANT_STEP, "running");
  try {
    await deps.grantOperator({
      channelId: launchChannelId,
      genesisRef,
      granteePubkey: lead.actor,
    });
    await deps.grantLeadSeat({
      channelId: launchChannelId,
      genesisRef,
      actorPubkey: lead.actor,
      role: "lead",
    });
  } catch (error) {
    return fail(
      CODING_SESSION_CREW_LAUNCH_GRANT_STEP,
      describe(
        error,
        `${lead.actorLabel} was seated but could not receive its operator and governed lead grants.`,
      ),
      sessionRef,
      genesisRef,
    );
  }
  // Never fatal: a session whose lead cannot publish the project's actions is
  // still a working session, so this is disclosed on the step rather than
  // failing a launch that has already seated the lead (ledger 186, 178(f)).
  let actionsDetail: string | null = null;
  if (deps.grantProjectActions && (input.projectRef ?? null) !== null) {
    try {
      actionsDetail = await deps.grantProjectActions({
        channelId: launchChannelId,
        genesisRef,
        actorPubkey: lead.actor,
        projectRef: input.projectRef as string,
        leadLabel: lead.actorLabel,
      });
    } catch (error) {
      actionsDetail = describe(
        error,
        codingSessionProjectActionsGrantDisclosure({
          leadLabel: lead.actorLabel,
          detail: null,
        }),
      );
    }
  }
  mark(CODING_SESSION_CREW_LAUNCH_GRANT_STEP, "done", actionsDetail);

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
        roster: input.hireRoster ?? null,
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
    leadWorkdir,
    policyEventId,
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
