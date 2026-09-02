/**
 * The one launch form's model: who leads, whether it is governed, and what
 * still stands between the person and pressing the button.
 *
 * The dialog used to be two tabs — *One session* and *Team* — which is two
 * answers to "what am I starting?" and, in practice, two half-forms. The Team
 * tab had no provider control at all, so the lead ran on whatever the other
 * tab was showing; the One-session tab's model leaked into unpinned seats
 * (item 103, finding 12); role names were free text; and the lead's name was
 * truncated to the point where two Keystones read identically. The ruling of
 * 2026-09-01 collapsed both into one form — goal · who leads · governed ·
 * bench · budget/posture · working directory · access — and this module holds
 * the parts of that form that are decisions rather than pixels.
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
  KIND_CODING_SESSION_GENESIS,
  KIND_CODING_SESSION_GOAL,
  KIND_CODING_SESSION_LIFECYCLE_COMMAND,
  KIND_CODING_SESSION_POLICY,
} from "@/shared/constants/kinds";
import { truncatePubkey } from "@/shared/lib/pubkey";

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
    };

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
  return `${lead.label} · ${truncatePubkey(lead.actor)}`;
}

/**
 * Whether this launch is governed — decided by who leads, never separately.
 *
 * An agent lead is always governed: a seat holds authority only through a
 * genesis and an accepted authority chain, so an ungoverned agent lead would
 * be an agent with no standing to report, to hire, or to be granted anything
 * — a session that looks founded and can do nothing.
 *
 * Leading it yourself is always ungoverned, and that is the honest half: a
 * governed session exists so that *somebody who is not you* can hold a seat in
 * it. Founding a roster with one member, yourself, and an authority chain
 * nobody else appears in would publish three extra records that answer no
 * question anybody asked. The switch is therefore shown and derived rather
 * than shown and free — a control that claims to decide something it does not
 * is the class of defect this form was rebuilt to remove.
 */
export function codingSessionLaunchIsGoverned(
  lead: CodingSessionLaunchLead,
): boolean {
  return lead.kind === "agent";
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
}): { model: string | null; overridden: boolean } {
  const clean = (value: string | null | undefined) => {
    const trimmed = value?.trim() ?? "";
    return trimmed.length > 0 ? trimmed : null;
  };
  if (input.lead.kind !== "agent") {
    return { model: clean(input.providerModel), overridden: false };
  }
  const identityModel = clean(input.lead.model);
  if (!input.pickedExplicitly) {
    return { model: identityModel, overridden: false };
  }
  const picked = clean(input.pickedModel);
  return { model: picked, overridden: picked !== identityModel };
}

/** Why the governed switch reads the way it does. Never absent. */
export function codingSessionGovernedLockReason(
  lead: CodingSessionLaunchLead,
): string {
  return lead.kind === "agent"
    ? "An agent lead is always governed: its seat, its grants and every report it files are answered against this session's genesis."
    : "You are leading, so there is nothing to govern yet. Pick an agent to lead and the launch founds a genesis and an authority chain.";
}

/** Something the person can act on right now. Rendered inline, never hidden. */
export type CodingSessionLaunchBlocker = {
  id: string;
  /** One sentence naming what is missing. */
  sentence: string;
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
 * Everything standing between this form and a signed genesis.
 *
 * Pure, so the button's disabled state and the sentence under it are the same
 * expression — a disabled control with nothing under it is the front door
 * refusing in silence (item 79), and the two drifting apart is how that
 * happens after the first fix.
 */
export function codingSessionLaunchReadiness(input: {
  /** Null when there is no channel and nothing here can publish one. */
  channelId: string | null;
  canCreateChannel: boolean;
  goal: string;
  /** The overflow the shared goal helper reports, or null when it fits. */
  goalOverflow: { bytes: number; cap: number } | null;
  lead: CodingSessionLaunchLead;
  governed: boolean;
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
  const blockers: CodingSessionLaunchBlocker[] = [];
  const unknowns: CodingSessionLaunchUnknown[] = [];

  // Truthiness rather than `!== null`: a caller that omits the field entirely
  // means "not busy", and an empty sentence is not a disclosure.
  if (input.busySentence) {
    blockers.push({ id: "busy", sentence: input.busySentence });
  }

  if (input.channelId === null && !input.canCreateChannel) {
    blockers.push({
      id: "channel",
      sentence:
        "This session has nowhere to launch into, and nothing here can create a channel for it.",
    });
  }
  if (input.goal.trim().length === 0) {
    blockers.push({
      id: "goal",
      sentence: "Write the goal — the lead's first turn carries it.",
    });
  }
  if (input.goalOverflow) {
    blockers.push({
      id: "goal-cap",
      sentence: `This goal is ${input.goalOverflow.bytes.toLocaleString()} UTF-8 bytes; the signed record holds ${input.goalOverflow.cap.toLocaleString()}.`,
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
  if (input.lead.kind === "agent" && input.leadModel === null) {
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
        "The policy below is published as a signed record and nothing in this build enforces it yet.",
    });
  }
  if (!input.governed) {
    unknowns.push({
      id: "ungoverned",
      sentence:
        "Ungoverned: no genesis and no authority chain, so this session has no roster, no grants and no signed reports.",
    });
  }

  return { blockers, unknowns, canLaunch: blockers.length === 0 };
}

/** One line of "here is what pressing this publishes". */
export type CodingSessionLaunchPlanLine = {
  id: string;
  /** The kind integer, so the sentence and the wire cannot drift. */
  kind: number;
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
  governed: boolean;
  lead: CodingSessionLaunchLead;
  goal: string;
  policySet: boolean;
  benchCount: number;
}): CodingSessionLaunchPlanLine[] {
  if (!input.governed) {
    return [
      {
        id: "create",
        kind: KIND_CODING_SESSION_LIFECYCLE_COMMAND,
        sentence: "One session, founded by you, with your first message.",
      },
    ];
  }
  const lines: CodingSessionLaunchPlanLine[] = [
    {
      id: "genesis",
      kind: KIND_CODING_SESSION_GENESIS,
      sentence: "Found the session (genesis).",
    },
    {
      id: "goal",
      kind: KIND_CODING_SESSION_GOAL,
      sentence: "Publish the goal as the session's own.",
    },
  ];
  if (input.policySet) {
    lines.push({
      id: "policy",
      kind: KIND_CODING_SESSION_POLICY,
      sentence: "Publish the policy — stated, not enforced.",
    });
  }
  lines.push({
    id: "create",
    kind: KIND_CODING_SESSION_LIFECYCLE_COMMAND,
    sentence:
      input.lead.kind === "agent"
        ? `Seat ${input.lead.label} as ${input.lead.role} — the only seat this launch creates.`
        : "Seat you as the lead — the only seat this launch creates.",
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
        ? `Send the goal and the ${input.benchCount === 1 ? "one identity" : `${input.benchCount} identities`} the lead may hire.`
        : "Send the goal. Nobody is benched, so the lead hires nobody.",
  });
  return lines;
}
