import {
  type CodingSessionLaunchLead,
  codingSessionLeadIdentityLine,
} from "../lib/codingSessionLaunchForm";

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
 */
export function NewCodingSessionLeadField({
  candidates,
  disabled = false,
  lead,
  onLeadChange,
}: {
  candidates: readonly NewCodingSessionLeadCandidate[];
  disabled?: boolean;
  lead: CodingSessionLaunchLead;
  onLeadChange: (actor: string | null) => void;
}) {
  const seatable = candidates.filter((candidate) => candidate.role !== null);
  return (
    <div className="flex flex-col gap-2" data-testid="new-coding-session-lead">
      <label
        className="text-xs font-medium text-muted-foreground"
        htmlFor="coding-session-lead"
      >
        Who leads
      </label>
      <select
        className="h-9 rounded-md border border-input bg-transparent px-3 text-sm disabled:opacity-50"
        data-testid="new-coding-session-lead-select"
        disabled={disabled}
        id="coding-session-lead"
        onChange={(event) =>
          onLeadChange(event.target.value === "" ? null : event.target.value)
        }
        value={lead.kind === "agent" ? lead.actor : ""}
      >
        <option value="">Pick an agent…</option>
        {seatable.map((candidate) => (
          <option key={candidate.pubkey} value={candidate.pubkey}>
            {candidate.name} · {candidate.role}
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
      {seatable.length === 0 ? (
        <p className="text-2xs text-muted-foreground">
          No identity on this computer carries a role, so there is nobody to
          lead a team. Install role packs from the project's{" "}
          <code>personas/roles</code>, or switch to Solo.
        </p>
      ) : null}
    </div>
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
