/**
 * The one launch form's model: Solo or Team, who leads, and what still stands
 * between the person and pressing Start.
 *
 * The dialog used to be two tabs — *One session* and *Team* — which is two
 * answers to "what am I starting?" and, in practice, two half-forms. The
 * ruling of 2026-09-01 collapsed both into one form, and 2026-09-08 moved the
 * team half onto the founded session's own screen behind an "Agent team" box.
 * Since 2026-09-10 there is no dialog at all: a click founds the session and
 * the founded page is the whole form, with a **Solo | Team** switch at the top
 * (Andy: "both normal and team sessions should be configured this way").
 * `governed` is that switch — Team means an agent leads — and this module
 * holds the parts of the form that are decisions rather than pixels.
 *
 * Two rules the shapes here exist to enforce:
 *
 * 1. **Only the lead is created at launch.** There is no roster of seats to
 *    create, so there is no seat whose model has to be guessed, so the leak
 *    has nowhere to land. Everyone else is bench: who the lead *may* hire.
 * 2. **Readiness shows blockers, and hides nothing.** A blocker is something
 *    a person can act on now and it is inline. An unknown is something this
 *    computer genuinely does not know, and it is behind a disclosure — shown,
 *    never dropped, because "we could not check" and "it is fine" are
 *    different facts and only one of them is an excuse for silence.
 */
import {
  KIND_CODING_SESSION_AUTHORITY_TRANSITION,
  KIND_CODING_SESSION_COMMAND,
  KIND_CODING_SESSION_GOAL,
  KIND_CODING_SESSION_LIFECYCLE_COMMAND,
  KIND_CODING_SESSION_NAME,
  KIND_CODING_SESSION_POLICY,
} from "@/shared/constants/kinds";
import { truncatePubkey } from "@/shared/lib/pubkey";
import { CODING_SESSION_GOAL_UNRESOLVED } from "./codingSessionGoal";
import type { CodingSessionSetupMode } from "./codingSessionSetupMode";

/** Who holds the founding seat of this session. */
export type CodingSessionLaunchLead =
  | {
      kind: "you";
      /** How the founder is named on screen. */
      label: string;
    }
  | {
      kind: "agent";
      /** Lowercase 64-hex pubkey of the managed agent. */
      actor: string;
      /** The identity's own name — never truncated. */
      label: string;
      /** Role slug read from the identity's pack, never typed by hand. */
      role: string;
      /** The model the identity itself publishes, or null when it names none. */
      model: string | null;
      /** Whether this computer holds the identity's role pack. */
      hasRolePack?: boolean;
    }
  /**
   * Team, with no agent picked yet. A variant rather than `null` so every
   * site that narrows on `lead.kind === "agent"` keeps compiling and correct;
   * readiness turns it into the `lead` blocker.
   */
  | { kind: "unset" };

/**
 * The lead's identity line: the whole name, then the canonical short pubkey.
 *
 * Two managed agents may carry the same display name — the crew installer
 * offers a name field per pack, and "Keystone" is the obvious thing to type
 * twice — and the launch form is exactly where picking the wrong one costs a
 * whole session. The short form is `truncatePubkey`'s, because a hand-rolled
 * slice is a forgeable prefix nobody audits (`check-pubkey-truncation`).
 */
export function codingSessionLeadIdentityLine(
  lead: CodingSessionLaunchLead,
): string {
  if (lead.kind === "you") return lead.label;
  if (lead.kind === "unset") return "No lead picked yet";
  return `${lead.label} · ${truncatePubkey(lead.actor)}`;
}

/**
 * The model the lead will actually be created on — from the identity, or from
 * an explicit pick, and from nowhere else.
 *
 * This is where finding 12 is actually closed. Removing `fallbackModel` from
 * `resolveCodingSessionCrewSeats` closed it on a function the form does not
 * call (REVIEW-B3 F1); the live path took `effectiveModel`, which is
 * `seatModel.model ?? providerModel`, and `providerModel` is the **picker's
 * default** whenever nobody has chosen by hand. So an identity that declared
 * no model was created on whatever the picker happened to be showing, and the
 * disclosure written for that case could never fire because the model was
 * never `null`.
 *
 * Two sources now, and a third state:
 *
 * - the identity's own model, when it declares one;
 * - an explicit pick, which is an **override** and owes a reason;
 * - otherwise `null`, which blocks the launch rather than borrowing a model.
 *
 * A blank string is `null`: a runtime whose models command has not answered
 * publishes `defaultModel: ""` (`newCodingSessionModel.ts` bootstrap), and an
 * empty model is not a model (REVIEW-B3 F2).
 *
 * Leading it yourself is different and deliberately so: the picker *is* your
 * choice, so its value is the model, and there is nothing to override.
 */
export function resolveCodingSessionLeadModel(input: {
  lead: CodingSessionLaunchLead;
  /** The model the person picked by hand, when they picked one. */
  pickedModel: string | null;
  pickedExplicitly: boolean;
  /** What the provider picker resolved — only ever used when you lead. */
  providerModel: string | null;
  /**
   * The disclosed stand-in the picker preselected because the runtime does
   * not offer the identity's own id (ledger 255(e)), or null. Running it is
   * not an override: the notice under the picker already names both ids.
   */
  seatSubstitute?: string | null;
}): { model: string | null; overridden: boolean } {
  const clean = (value: string | null | undefined) => {
    const trimmed = value?.trim() ?? "";
    return trimmed.length > 0 ? trimmed : null;
  };
  if (input.lead.kind !== "agent") {
    return { model: clean(input.providerModel), overridden: false };
  }
  const identityModel = clean(input.lead.model);
  const substitute = clean(input.seatSubstitute);
  if (!input.pickedExplicitly) {
    return { model: substitute ?? identityModel, overridden: false };
  }
  const picked = clean(input.pickedModel);
  return {
    model: picked,
    overridden: picked !== identityModel && picked !== substitute,
  };
}

/** Something the person can act on right now. Rendered inline, never hidden. */
export type CodingSessionLaunchBlocker = {
  id: string;
  /** One sentence naming what is missing. */
  sentence: string;
  /**
   * When the sentence is shown. Absent means inline, before any press, and
   * the button is disabled by it. `attempt` means the button stays pressable
   * and the sentence appears only once somebody presses it — the blank
   * initial prompt (Andy, 2026-09-10): a fresh page reads as a form, not
   * as a refusal. `canLaunch` is false either way; nothing is signed.
   */
  surface?: "attempt";
};

/**
 * Something this computer could not determine.
 *
 * Not a blocker and not nothing: it is disclosed behind "Details" so the
 * launch is not held up by a question nobody can answer, and so a person who
 * wants to know what was *not* checked can find out without reading the code.
 */
export type CodingSessionLaunchUnknown = {
  id: string;
  sentence: string;
};

export type CodingSessionLaunchReadiness = {
  blockers: CodingSessionLaunchBlocker[];
  unknowns: CodingSessionLaunchUnknown[];
  /** True exactly when there are no blockers. Unknowns never block. */
  canLaunch: boolean;
};

/**
 * The blockers by where they show: `inline` ones sit under the button and
 * disable it; `onAttempt` ones wait for a press. `canPress` is "no inline
 * blocker" — the button's own enabled state — and is deliberately not
 * `canLaunch`, which stays false while any blocker stands.
 */
export function codingSessionLaunchBlockersBySurface(
  readiness: Pick<CodingSessionLaunchReadiness, "blockers">,
): {
  inline: CodingSessionLaunchBlocker[];
  onAttempt: CodingSessionLaunchBlocker[];
  canPress: boolean;
} {
  const inline = readiness.blockers.filter(
    (blocker) => blocker.surface !== "attempt",
  );
  const onAttempt = readiness.blockers.filter(
    (blocker) => blocker.surface === "attempt",
  );
  return { inline, onAttempt, canPress: inline.length === 0 };
}

/** The goal reader's own condition, as the founded page derives it. */
export type CodingSessionLaunchGoalReader =
  | "unresolved"
  | "errored"
  | "resolved";

/** The names reader's condition — `useCodingSessionNames().resolved`. */
export type CodingSessionLaunchNameReader = "unresolved" | "resolved";

/** The sentence under a Team Start with no agent picked. */
export const CODING_SESSION_LAUNCH_LEAD_BLOCKER =
  "Pick an agent to lead this session, or switch to Solo.";
/** The on-press sentence for a worktree left unnamed with the toggle on. */
export const CODING_SESSION_LAUNCH_WORKTREE_NAME_BLOCKER =
  "Give the worktree a name — letters and numbers — or turn the worktree off.";

/**
 * Everything standing between this form and a signed create.
 *
 * Pure, so the button's disabled state and the sentence under it are the same
 * expression — a disabled control with nothing under it is the front door
 * refusing in silence (item 79), and the two drifting apart is how that
 * happens after the first fix.
 */
export function codingSessionLaunchReadiness(input: {
  /** Solo: you lead, one runtime. Team: an agent leads; governed. */
  mode: CodingSessionSetupMode;
  /** The initial prompt as the field holds it — draft or wire. */
  goal: string;
  /** The overflow the shared goal helper reports, or null when it fits. */
  goalOverflow: { bytes: number; cap: number } | null;
  /**
   * Whether the wire goal has been read. A Start must not republish or
   * shadow a goal it cannot see, so anything but `resolved` blocks.
   */
  goalReader: CodingSessionLaunchGoalReader;
  /**
   * Whether the wire name has been read. Read only when `nameDirty`: a Start
   * flushes a dirty name first, and it must not publish over — or silently
   * drop — a name it has not seen.
   */
  nameReader: CodingSessionLaunchNameReader;
  /** The worktree toggle and its name: a blank name with the toggle on is
   * refused on press, under the field, rather than by a failed cut. */
  useWorktree?: boolean;
  worktreeName?: string;
  /** The Name field differs from the wire, so Start would publish a 44229. */
  nameDirty: boolean;
  lead: CodingSessionLaunchLead;
  /** Null when no installed, authenticated runtime is selected. */
  providerInstanceRef: string | null;
  providerAuthorityPubkey: string | null;
  /** The model the lead will actually run on, or null when none is settled. */
  leadModel: string | null;
  /** Set when the person overrode the routed model; a reason is then required. */
  modelOverrideReason: string | null;
  modelOverridden: boolean;
  /** A refusal the provider/vendor checks already produced, or null. */
  providerRefusal: string | null;
  /** Team readiness: `null` means nothing was asked (no project). */
  projectReadiness: { allowed: boolean; reason: string | null } | null;
  /** True while a readiness read is still in flight or errored. */
  projectReadinessUnknown: boolean;
  /** `undefined` when nobody asked this computer about the lead's role pack. */
  leadHasRolePack?: boolean;
  /** True when the form set at least one 44245 field. */
  policySet: boolean;
  /** Identities benched but not resolvable to a managed agent here. */
  unresolvedBenchIdentities: readonly string[];
  /**
   * Work already in flight that must finish first, as the sentence a person
   * reads. Null when nothing is in flight.
   *
   * Held here rather than `&&`-ed onto the button so that *every* disabled
   * state has a sentence under it. It was not: during channel preparation and
   * the readiness preflight the button went dead with nothing under it, which
   * is precisely the item-79 shape this form claims to have removed
   * (REVIEW-B3 F7).
   */
  busySentence: string | null;
}): CodingSessionLaunchReadiness {
  const governed = input.mode === "team";
  const blockers: CodingSessionLaunchBlocker[] = [];
  const unknowns: CodingSessionLaunchUnknown[] = [];

  // Truthiness rather than `!== null`: a caller that omits the field entirely
  // means "not busy", and an empty sentence is not a disclosure.
  if (input.busySentence) {
    blockers.push({ id: "busy", sentence: input.busySentence });
  }

  // Surfaced on press, not inline: see `CodingSessionLaunchBlocker.surface`.
  if (input.goal.trim().length === 0) {
    blockers.push({
      id: "goal",
      sentence: "Please specify the initial prompt to start the session.",
      surface: "attempt",
    });
  }
  if (input.goalOverflow) {
    blockers.push({
      id: "goal-cap",
      sentence: `This goal is ${input.goalOverflow.bytes.toLocaleString()} UTF-8 bytes; the signed record holds ${input.goalOverflow.cap.toLocaleString()}.`,
    });
  }
  if (input.goalReader !== "resolved") {
    blockers.push({
      id: "goal-unresolved",
      sentence:
        input.goalReader === "unresolved"
          ? `${CODING_SESSION_GOAL_UNRESOLVED} Start waits until the relay has answered, so it cannot publish over an initial prompt it has not seen.`
          : "The session's goal could not be read from the relay, so Start cannot tell what is already published and will not send or replace it.",
    });
  }
  if (input.nameDirty && input.nameReader !== "resolved") {
    blockers.push({
      id: "name-unresolved",
      sentence:
        "Name not read yet. Start waits until the relay has answered, so it cannot publish over a name it has not seen.",
    });
  }
  // Surfaced on press, like the blank prompt: a Team page with nobody
  // picked yet is a page being filled in, not a refusal (Andy, 2026-09-10).
  if (governed && input.lead.kind !== "agent") {
    blockers.push({
      id: "lead",
      sentence: CODING_SESSION_LAUNCH_LEAD_BLOCKER,
      surface: "attempt",
    });
  }
  if (input.useWorktree && (input.worktreeName ?? "").trim().length === 0) {
    blockers.push({
      id: "worktree-name",
      sentence: CODING_SESSION_LAUNCH_WORKTREE_NAME_BLOCKER,
      surface: "attempt",
    });
  }
  if (
    input.providerInstanceRef === null ||
    input.providerAuthorityPubkey === null
  ) {
    blockers.push({
      id: "provider",
      sentence:
        "No coding-session provider is available on this computer, so there is nothing to run the lead on.",
    });
  }
  if (input.providerRefusal) {
    blockers.push({ id: "provider-refusal", sentence: input.providerRefusal });
  }
  if (input.modelOverridden && !input.modelOverrideReason?.trim()) {
    blockers.push({
      id: "override-reason",
      // The same thing `bee sessions hire --override-model` requires: an
      // override with no reason is a decision nobody can audit later.
      sentence:
        "Say why this model overrides the routed one — an override with no reason is a decision nobody can account for.",
    });
  }
  if (input.projectReadiness && !input.projectReadiness.allowed) {
    blockers.push({
      id: "project-readiness",
      sentence:
        input.projectReadiness.reason ?? "The project is not prepared yet.",
    });
  }
  for (const identity of input.unresolvedBenchIdentities) {
    blockers.push({
      id: `bench:${identity}`,
      sentence: `${truncatePubkey(identity)} is on the bench but is not an identity this computer manages, so the lead could not hire it.`,
    });
  }

  if (input.projectReadinessUnknown) {
    unknowns.push({
      id: "project-readiness",
      sentence:
        "This computer could not read the project's team readiness, so nothing here has checked it.",
    });
  }
  if (input.lead.kind === "agent" && input.leadHasRolePack === undefined) {
    unknowns.push({
      id: "role-pack",
      sentence:
        "Nobody asked whether this computer holds the lead's role pack, so whether it will carry its role's craft is unknown.",
    });
  }
  if (input.lead.kind === "agent" && input.leadHasRolePack === false) {
    unknowns.push({
      id: "role-pack-missing",
      sentence:
        "This computer holds no role pack behind the lead, so it runs on its persona prompt alone.",
    });
  }
  // Team, an agent lead, no model: a blocker, never for `unset` (the `lead`
  // blocker already names what to do) and never in Solo.
  if (governed && input.lead.kind === "agent" && input.leadModel === null) {
    blockers.push({
      id: "model",
      // A blocker, not an unknown. The alternative — publishing the picker's
      // default for an identity that declared none — is a seat running weights
      // nobody chose, recorded on the wire as though somebody had (REVIEW-B3
      // F1). Picking one here is an override and owes its reason.
      sentence: `${input.lead.label} declares no model and nothing has routed one, so there is no model to create it on. Pick one — an explicit pick is recorded as an override.`,
    });
  }
  if (input.lead.kind === "you" && input.leadModel === null) {
    unknowns.push({
      id: "model",
      sentence:
        "No model is settled, so this session runs on the runtime's own default and the create records none.",
    });
  }
  if (input.policySet) {
    unknowns.push({
      id: "policy",
      sentence:
        "The policy below is published as a signed record. Turn budgets and verifier/required-gate settings have scoped consumers; the other fields are read and shown as guidance.",
    });
  }
  if (!governed) {
    // Corrected 2026-09-10: a founded session has a genesis; what an
    // ungoverned session lacks is the authority chain.
    unknowns.push({
      id: "ungoverned",
      sentence:
        "Ungoverned: no authority chain, so this session has no roster, no grants and no signed reports.",
    });
  }

  return { blockers, unknowns, canLaunch: blockers.length === 0 };
}

/** One line of "here is what pressing this publishes". */
export type CodingSessionLaunchPlanLine = {
  id: string;
  /**
   * The kind integer, so the sentence and the wire cannot drift — or null for
   * a line that publishes nothing, because a kind on that line would claim a
   * record that never goes out. Every line today names a kind.
   */
  kind: number | null;
  sentence: string;
};

/**
 * What the launch will actually publish, said before it is pressed.
 *
 * A screen listing four roles beside a button called "Launch team" implied
 * four agents start; the same class of mistake is a button called "Launch"
 * over a form with a bench, a policy and a governed switch. Naming the events
 * — with their kind integers — is the only version of this sentence that
 * cannot quietly stop being true.
 */
export function codingSessionLaunchPlan(input: {
  mode: CodingSessionSetupMode;
  lead: CodingSessionLaunchLead;
  /** The Name field differs from the wire, so Start publishes a 44229 first. */
  nameDirty: boolean;
  /** The prompt differs from the wire goal, so Start publishes a 44227. */
  promptDirty: boolean;
  policySet: boolean;
  benchCount: number;
}): CodingSessionLaunchPlanLine[] {
  const lines: CodingSessionLaunchPlanLine[] = [];
  // The two text fields flush before either branch, only when dirty — a line
  // for an unchanged field would name a record Start does not publish.
  if (input.nameDirty) {
    lines.push({
      id: "name",
      kind: KIND_CODING_SESSION_NAME,
      sentence: "Publish the name.",
    });
  }
  if (input.promptDirty) {
    lines.push({
      id: "goal",
      kind: KIND_CODING_SESSION_GOAL,
      sentence: "Publish the initial prompt as the session's goal.",
    });
  }
  if (input.mode === "solo") {
    lines.push({
      id: "create",
      kind: KIND_CODING_SESSION_LIFECYCLE_COMMAND,
      sentence:
        "One execution under this session, led by you, with the initial prompt as its first message.",
    });
    return lines;
  }
  if (input.policySet) {
    lines.push({
      id: "policy",
      kind: KIND_CODING_SESSION_POLICY,
      sentence:
        "Publish the policy — turn budgets and verifier gates have scoped effects; other fields guide the lead.",
    });
  }
  lines.push({
    id: "create",
    kind: KIND_CODING_SESSION_LIFECYCLE_COMMAND,
    sentence:
      input.lead.kind === "agent"
        ? `Seat ${input.lead.label} as ${input.lead.role} — the only seat this launch creates.`
        : "Seat the agent picked above as the lead — the only seat this launch creates.",
  });
  lines.push({
    id: "grants",
    kind: KIND_CODING_SESSION_AUTHORITY_TRANSITION,
    sentence: "Grant the lead operator authority and its role seat.",
  });
  lines.push({
    id: "turn",
    kind: KIND_CODING_SESSION_COMMAND,
    sentence:
      input.benchCount > 0
        ? `Send the initial prompt and the ${input.benchCount === 1 ? "one identity" : `${input.benchCount} identities`} the lead may hire.`
        : "Send the initial prompt. Nobody is benched, so the lead hires nobody.",
  });
  return lines;
}
