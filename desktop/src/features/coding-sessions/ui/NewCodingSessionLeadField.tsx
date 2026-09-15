import {
  type CodingSessionLaunchLead,
  codingSessionLeadIdentityLine,
} from "../lib/codingSessionLaunchForm";
import {
  type CodingSessionCandidateGroup,
  codingSessionCandidateOptionLabel,
} from "../lib/codingSessionLeadCandidateGroups";

/** A managed agent this computer could seat as the lead. */
export type NewCodingSessionLeadCandidate = {
  pubkey: string;
  name: string;
  /**
   * The role this identity *is*, read from its pack. Never typed by hand:
   * free-text role names were item 103's finding 12, and a role slug the
   * relay refuses is a launch that dies after the genesis is already signed.
   */
  role: string | null;
  model: string | null;
  hasRolePack?: boolean;
  /**
   * The project this agent durably belongs to (`ManagedAgent.projectRef`), or
   * null when none. The only fact that decides whether it may lead here.
   */
  projectRef?: string | null;
};

/**
 * Who leads this Team session — one seated agent.
 *
 * Team means an agent leads (Andy, 2026-09-10); leading it yourself is the
 * Solo switch above this field, so "You" is not an option here. Nothing
 * picked is a placeholder, and readiness turns it into the `lead` blocker
 * rather than letting a session start with nobody in the seat.
 *
 * The identity line carries the whole name **and** the canonical short pubkey,
 * because two managed agents can be called Keystone and this is the screen
 * where picking the wrong one costs a session.
 *
 * The options are only the agents this session may seat: a project session's
 * own agents (by `ManagedAgent.projectRef`, never by role name), or, outside a
 * project, agents that belong to none. Whoever was left out is counted in one
 * sentence under the field, with the way to change it — never silently
 * dropped, and never listed as a choice the host would then refuse.
 */
export function NewCodingSessionLeadField({
  candidates,
  groups,
  disabled = false,
  emptySentence = null,
  exclusionSentence = null,
  lead,
  onLeadChange,
  onOpenProjectAgents = null,
}: {
  /** Every candidate the options were drawn from. */
  candidates: readonly NewCodingSessionLeadCandidate[];
  /** Grouped options; absent, the seatable candidates as one flat list. */
  groups?: readonly CodingSessionCandidateGroup<NewCodingSessionLeadCandidate>[];
  disabled?: boolean;
  /** Said instead of the default when there is nobody to pick. */
  emptySentence?: string | null;
  /** How many agents on this computer were left out, and why. */
  exclusionSentence?: string | null;
  lead: CodingSessionLaunchLead;
  onLeadChange: (actor: string | null) => void;
  /** Opens the session's project on its Agents tab, when there is one. */
  onOpenProjectAgents?: (() => void) | null;
}) {
  const options = groups
    ? groups.flatMap((group) => group.candidates)
    : candidates.filter((candidate) => candidate.role !== null);
  const labelled = groups?.some((group) => group.heading !== null) ?? false;
  return (
    <div className="flex flex-col gap-2" data-testid="new-coding-session-lead">
      <label
        className="text-xs font-medium text-muted-foreground"
        htmlFor="coding-session-lead"
      >
        Who leads
      </label>
      <select
        className="h-9 min-w-0 rounded-md border border-input bg-transparent px-3 text-sm disabled:opacity-50"
        data-testid="new-coding-session-lead-select"
        disabled={disabled}
        id="coding-session-lead"
        onChange={(event) =>
          onLeadChange(event.target.value === "" ? null : event.target.value)
        }
        value={lead.kind === "agent" ? lead.actor : ""}
      >
        <option value="">Pick an agent…</option>
        {labelled && groups
          ? groups.map((group) => (
              <optgroup
                data-testid={`new-coding-session-lead-group-${group.id}`}
                key={group.id}
                label={group.heading ?? ""}
              >
                {group.candidates.map((candidate) => (
                  <option key={candidate.pubkey} value={candidate.pubkey}>
                    {codingSessionCandidateOptionLabel(candidate)}
                  </option>
                ))}
              </optgroup>
            ))
          : options.map((candidate) => (
              <option key={candidate.pubkey} value={candidate.pubkey}>
                {codingSessionCandidateOptionLabel(candidate)}
              </option>
            ))}
      </select>
      <p
        className="text-2xs text-muted-foreground"
        data-testid="new-coding-session-lead-identity"
      >
        {codingSessionLeadIdentityLine(lead)}
        {lead.kind === "agent" ? ` · ${lead.role}` : ""}
        {lead.kind === "agent"
          ? lead.model
            ? ` · ${lead.model}`
            : " · no model of its own"
          : ""}
      </p>
      {options.length === 0 ? (
        <p
          className="text-2xs text-muted-foreground"
          data-testid="new-coding-session-lead-empty"
        >
          {emptySentence ?? (
            <>
              No identity on this computer carries a role, so there is nobody to
              lead a team. Install role packs from the project's{" "}
              <code>personas/roles</code>, or switch to Solo.
            </>
          )}
          {onOpenProjectAgents && !exclusionSentence ? (
            <>
              {" "}
              <OpenProjectAgentsButton onClick={onOpenProjectAgents} />
            </>
          ) : null}
        </p>
      ) : null}
      {exclusionSentence ? (
        <p
          className="text-2xs text-muted-foreground"
          data-testid="new-coding-session-lead-excluded"
        >
          {exclusionSentence}
          {onOpenProjectAgents ? (
            <>
              {" "}
              <OpenProjectAgentsButton onClick={onOpenProjectAgents} />
            </>
          ) : null}
        </p>
      ) : null}
    </div>
  );
}

function OpenProjectAgentsButton({ onClick }: { onClick: () => void }) {
  return (
    <button
      className="text-primary underline underline-offset-2"
      data-testid="new-coding-session-lead-open-agents"
      onClick={onClick}
      type="button"
    >
      Open the Agents tab
    </button>
  );
}

/**
 * Turn the picked pubkey into the lead the rest of the form reads.
 *
 * No fallback to "you": that is the Solo mode's answer, made by the setup
 * hook. Nothing picked — or a stale actor no longer among the candidates —
 * is `unset`, which readiness names rather than seating anybody by default.
 */
export function resolveNewCodingSessionLead(input: {
  actor: string | null;
  candidates: readonly NewCodingSessionLeadCandidate[];
}): CodingSessionLaunchLead {
  const candidate =
    input.actor === null
      ? null
      : (input.candidates.find(
          (entry) => entry.pubkey === input.actor && entry.role !== null,
        ) ?? null);
  if (!candidate || candidate.role === null) {
    return { kind: "unset" };
  }
  return {
    kind: "agent",
    actor: candidate.pubkey.toLowerCase(),
    label: candidate.name,
    role: candidate.role,
    model: candidate.model,
    ...(candidate.hasRolePack === undefined
      ? {}
      : { hasRolePack: candidate.hasRolePack }),
  };
}
